#!/usr/bin/env python3
"""Bake Voxygen GLSL stages offline with glslc targeting Vulkan 1.1."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCES = ROOT / "assets/voxygen/shaders"
if not SOURCES.is_dir():
    SOURCES = ROOT.parent / "voxy-assets/voxygen/shaders"
OUT = ROOT / "shaderbin"
INCLUDE = re.compile(r"(?m)^#include +<(.+)>\s*$")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(source):
    return "\n".join(line.strip() for line in source.splitlines() if line.strip())


def bake():
    pipeline = (ROOT / "src/render/renderer/pipeline_creation.rs").read_text()
    entries = re.findall(r'create_shader\(\s*"([\w.-]+)",\s*ShaderStage::(Vertex|Fragment)', pipeline)
    entries.append(("fluid-frag.shiny", "Fragment"))
    publications = []
    dependencies = {str(p.relative_to(SOURCES)): digest(p.read_bytes())
                    for p in sorted(SOURCES.rglob("*.glsl")) if p.relative_to(SOURCES).as_posix() not in ("include/cloud/regular.glsl", "include/cloud/flat.glsl")}
    with tempfile.TemporaryDirectory() as temporary:
        work = Path(temporary)
        for profile in ("default-map", "default-cheap", "minimal", "trueos-bringup", "flat-cloud-layer"):
            minimal = profile in ("minimal", "trueos-bringup")
            shadow = "NONE" if minimal else profile.split("-")[1].upper()
            defines = {
                "VOXYGEN_COMPUTATION_PREFERENCE": "VOXYGEN_COMPUTATION_PREFERENCE_FRAGMENT",
                "FLUID_MODE": "FLUID_MODE_MEDIUM", "CLOUD_MODE": "CLOUD_MODE_FLAT",
                "REFLECTION_MODE": "REFLECTION_MODE_HIGH",
                "LIGHTING_ALGORITHM": "LIGHTING_ALGORITHM_BLINN_PHONG",
                "SHADOW_MODE": f"SHADOW_MODE_{shadow}", "POINT_GLOW_FACTOR": "0.35",
                "FLASHING_LIGHTS_ENABLED": "", "RAIN_ENABLED": "",
                "BLOOM_FACTOR": "0.2", "BLOOM_UNIFORM_BLUR": "false",
            }
            if minimal:
                defines.update(FLUID_MODE="FLUID_MODE_LOW", CLOUD_MODE="CLOUD_MODE_FLAT",
                               REFLECTION_MODE="REFLECTION_MODE_LOW",
                               LIGHTING_ALGORITHM="LIGHTING_ALGORITHM_LAMBERTIAN")
                for key in ("POINT_GLOW_FACTOR", "BLOOM_FACTOR", "BLOOM_UNIFORM_BLUR"):
                    del defines[key]
            if profile == "trueos-bringup":
                settings = json.loads((ROOT / "trueos-bringup-profile.json").read_text())["graphics"]["render_mode"]
                assert settings["aa"] == "None" and settings["cloud"] == "Flat"
                for toggle, define in (("rain_enabled", "RAIN_ENABLED"), ("flashing_lights_enabled", "FLASHING_LIGHTS_ENABLED")):
                    if not settings[toggle]:
                        defines.pop(define, None)
                for shader in sorted(settings["experimental_shaders"]):
                    defines["EXPERIMENTAL_" + shader.upper()] = ""
            if profile == "flat-cloud-layer":
                defines = {
                    "VOXYGEN_COMPUTATION_PREFERENCE": "VOXYGEN_COMPUTATION_PREFERENCE_FRAGMENT",
                    "FLUID_MODE": "FLUID_MODE_LOW", "CLOUD_MODE": "CLOUD_MODE_FLAT",
                    "REFLECTION_MODE": "REFLECTION_MODE_LOW",
                    "LIGHTING_ALGORITHM": "LIGHTING_ALGORITHM_LAMBERTIAN",
                    "SHADOW_MODE": "SHADOW_MODE_NONE",
                }
            constants = (SOURCES / "include/constants.glsl").read_text()
            constants += "\n" + "\n".join(f"#define {k} {v}".rstrip() for k, v in defines.items()) + "\n"

            def expand(source, stack=()):
                def resolve(match):
                    name = match[1]
                    if name in stack:
                        raise ValueError(f"recursive include: {stack} -> {name}")
                    if name == "constants.glsl":
                        text = constants
                    else:
                        relative = {"anti-aliasing.glsl": "antialias/none.glsl" if profile == "trueos-bringup" else ("antialias/fxupscale.glsl" if minimal else "antialias/fxaa.glsl"),
                                    "cloud.glsl": "@cloud-flat"}.get(name, f"include/{name}")
                        text = (OUT / "cloud-flat.glsl").read_text() if relative == "@cloud-flat" else (SOURCES / relative).read_text()
                    return expand(text.rstrip("\n"), (*stack, name))
                return INCLUDE.sub(resolve, source)

            stage_entries = [("clouds-vert", "Vertex"), ("cloud-flat-layer-frag", "Fragment")] if profile == "flat-cloud-layer" else entries
            for name, stage in stage_entries:
                if minimal and name == "fluid-frag.shiny":
                    name = "fluid-frag.cheap"
                for display_color in ((False, True) if name == "postprocess-frag" else (False,)):
                    path = OUT / (name + ".glsl") if name in ("postprocess-frag", "cloud-flat-layer-frag") else SOURCES / (name.replace(".", "/") + ".glsl")
                    source = expand(path.read_text())
                    if display_color:
                        source = source.replace("#version 440 core", "#version 440 core\n#define TRUEOS_DISPLAY_COLOR", 1)
                    suffix = "vert" if stage == "Vertex" else "frag"
                    artifact_profile = profile if minimal or profile == "flat-cloud-layer" else shadow.lower()
                    filename = f"{name}.{artifact_profile}{'.display' if display_color else ''}.spv"
                    glsl = work / "shader.glsl"
                    glsl.write_text(source)
                    binary = work / filename
                    command = ["glslc", "--target-env=vulkan1.1", "-std=430core", "-O",
                               f"-fshader-stage={suffix}", str(glsl), "-o", str(binary)]
                    subprocess.run(command, check=True)
                    data = binary.read_bytes()
                    (OUT / filename).write_bytes(data)
                    publications.append(dict(name=name, stage=suffix, profile=profile,
                                             file=filename, sha256=digest(data), bytes=len(data),
                                             source_sha256=digest(canonical(source).encode())))
    manifest = dict(schema=1, format="spirv-vulkan1.1", native_execution_admitted=False,
                    compiler=subprocess.check_output(["glslc", "--version"], text=True).strip(),
                    options=["--target-env=vulkan1.1", "-std=430core", "-O"],
                    profiles=["minimal", "default-map", "default-cheap", "trueos-bringup", "flat-cloud-layer"], cloud_mode="Flat",
                    dependencies=dependencies, overrides={"cloud-flat-layer-frag.glsl": digest((OUT / "cloud-flat-layer-frag.glsl").read_bytes()), "cloud-flat.glsl": digest((OUT / "cloud-flat.glsl").read_bytes()), "postprocess-frag.glsl": digest((OUT / "postprocess-frag.glsl").read_bytes()), "../trueos-bringup-profile.json": digest((ROOT / "trueos-bringup-profile.json").read_bytes())}, shaders=publications)
    (OUT / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    rows = ["// Generated by shaderbin/bake_shaders.py.",
            "pub(super) const SHADERS: &[(&str, &str, &str, &[u8])] = &["]
    for item in publications:
        rows.append(f'    ("{item["name"]}", "{item["source_sha256"]}", "{item["sha256"]}", include_bytes!("{item["file"]}")),')
    rows.append("];\n")
    (OUT / "catalog.rs").write_text("\n".join(rows))
    verify()


def verify():
    manifest = json.loads((OUT / "manifest.json").read_text())
    for name, expected in manifest["dependencies"].items():
        assert digest((SOURCES / name).read_bytes()) == expected, f"stale source: {name}"
    for name, expected in manifest.get("overrides", {}).items():
        assert digest((OUT / name).read_bytes()) == expected, f"stale override: {name}"
    for item in manifest["shaders"]:
        data = (OUT / item["file"]).read_bytes()
        assert digest(data) == item["sha256"], f"corrupt artifact: {item['file']}"
        assert len(data) == item["bytes"] and len(data) % 4 == 0
        assert data[:4] == bytes.fromhex("03022307")
    print(f"Verified {len(manifest['shaders'])} shader artifacts")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    verify() if args.verify else bake()
