#!/usr/bin/env python3
"""Capture the exact Voxy bring-up skybox SPIR-V as ADL-S native stages.

Compilation only: the mandatory no-op DRM shim prevents GPU submission.
This does not admit a runtime device or claim a displayed frame.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

VOXY = Path(__file__).resolve().parents[1]
TRUEOS = VOXY.parent / "TRUEOS"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def load_baker():
    spec = importlib.util.spec_from_file_location(
        "native_baker", TRUEOS / "tools/helio-intel-bake/bake.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def compile_source(baker):
    source = baker.UPSTREAM_DUMPER.read_text()
    # End before command recording: capture compilation, never run a triangle.
    end = source.index("    const float ordinary_vertices[9] = {")
    source = source[:end] + '    puts("voxy_skybox: compiled_only=1");\n    return 0;\n}\n'
    start = source.index("    const VkAttachmentDescription attachment = {")
    end = source.index("    FileData vs_spirv = read_spirv(argv[1]);", start)
    source = source[:start] + '''
    const VkAttachmentDescription attachments[3] = {
        { .format = VK_FORMAT_R16G16B16A16_SFLOAT, .samples = VK_SAMPLE_COUNT_1_BIT,
          .loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR, .storeOp = VK_ATTACHMENT_STORE_OP_STORE,
          .initialLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
          .finalLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL },
        { .format = VK_FORMAT_R8G8B8A8_UINT, .samples = VK_SAMPLE_COUNT_1_BIT,
          .loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR, .storeOp = VK_ATTACHMENT_STORE_OP_STORE,
          .initialLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
          .finalLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL },
        { .format = VK_FORMAT_D32_SFLOAT, .samples = VK_SAMPLE_COUNT_1_BIT,
          .loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR, .storeOp = VK_ATTACHMENT_STORE_OP_STORE,
          .initialLayout = VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
          .finalLayout = VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL },
    };
    const VkAttachmentReference colors[2] = {
        { 0, VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL },
        { 1, VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL },
    };
    const VkAttachmentReference depth_ref = { 2, VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL };
    const VkSubpassDescription subpass = {
        .pipelineBindPoint = VK_PIPELINE_BIND_POINT_GRAPHICS,
        .colorAttachmentCount = 2, .pColorAttachments = colors,
        .pDepthStencilAttachment = &depth_ref,
    };
    const VkRenderPassCreateInfo render_pass_info = {
        .sType = VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
        .attachmentCount = 3, .pAttachments = attachments,
        .subpassCount = 1, .pSubpasses = &subpass,
    };
    VkRenderPass render_pass;
    CHECK_VK(vkCreateRenderPass(device, &render_pass_info, NULL, &render_pass));
''' + source[end:]
    source = baker.replace_once(source, ".cullMode = VK_CULL_MODE_NONE,",
                                ".cullMode = VK_CULL_MODE_BACK_BIT,")
    start = source.index("    const VkPipelineColorBlendAttachmentState blend_attachment = {")
    end = source.index("    VkPipelineLayout pipeline_layout;", start)
    source = source[:start] + '''
    const VkPipelineColorBlendAttachmentState blend_attachments[2] = {
        { .blendEnable = VK_TRUE,
          .srcColorBlendFactor = VK_BLEND_FACTOR_ONE, .dstColorBlendFactor = VK_BLEND_FACTOR_ZERO,
          .colorBlendOp = VK_BLEND_OP_ADD,
          .srcAlphaBlendFactor = VK_BLEND_FACTOR_ZERO, .dstAlphaBlendFactor = VK_BLEND_FACTOR_ONE,
          .alphaBlendOp = VK_BLEND_OP_ADD, .colorWriteMask = 15 },
        { .blendEnable = VK_FALSE, .colorWriteMask = 15 },
    };
    const VkPipelineColorBlendStateCreateInfo blend = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO,
        .attachmentCount = 2, .pAttachments = blend_attachments,
    };
    const VkPipelineDepthStencilStateCreateInfo depth = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_DEPTH_STENCIL_STATE_CREATE_INFO,
        .depthTestEnable = VK_TRUE, .depthWriteEnable = VK_TRUE,
        .depthCompareOp = VK_COMPARE_OP_GREATER_OR_EQUAL,
    };
    const VkDescriptorSetLayoutBinding globals = {
        .binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
        .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT | VK_SHADER_STAGE_FRAGMENT_BIT,
    };
    const VkDescriptorSetLayoutCreateInfo globals_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 1, .pBindings = &globals,
    };
    VkDescriptorSetLayout globals_layout;
    CHECK_VK(vkCreateDescriptorSetLayout(device, &globals_info, NULL, &globals_layout));
    const VkPipelineLayoutCreateInfo pipeline_layout_info = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        .setLayoutCount = 1, .pSetLayouts = &globals_layout,
    };
''' + source[end:]
    source = baker.replace_once(source, ".pMultisampleState = &multisample,",
                                ".pMultisampleState = &multisample,\n        .pDepthStencilState = &depth,")
    return source


def state(path):
    if not path.is_file():
        raise SystemExit(f"instrumented Mesa stage capture missing: {path}")
    return {k: int(v, 0) for k, v in re.findall(
        r"(\w+)=(0x[0-9a-fA-F]+|\d+)", path.read_text().splitlines()[0])}


def push_ranges(path):
    ranges = []
    for line in path.read_text().splitlines():
        if line.startswith("TRUEOS_PUSH_RANGE "):
            values = {k: int(v) for k, v in re.findall(r"(\w+)=(\d+)", line)}
            if values["length"]:
                ranges.append(values)
    if len(ranges) != 1 or ranges[0]["set"] != 0 or ranges[0]["index"] != 0:
        raise SystemExit(f"expected one captured Globals push range: {path}; apply mesa-skybox-push-capture.patch")
    return ranges


def decode(path):
    result = subprocess.run(["iga64", "-d", "-p=12p1", "-Xprint-pc", str(path)],
                            capture_output=True, text=True)
    if result.returncode or "illegal" in result.stdout.lower() or "{EOT" not in result.stdout:
        raise SystemExit(f"incomplete or invalid native stage: {path}\n{result.stdout}{result.stderr}")
    return result.stdout


def verify(out, source_stages):
    metadata = json.loads((out / "metadata.json").read_text())
    if metadata["source_stages"] != [item for _, item in source_stages.values()]:
        raise SystemExit("native skybox package is stale; rebake from the current SPIR-V")
    if metadata["target"]["device_id"] != 0x4680:
        raise SystemExit("native skybox target mismatch")
    for name, record in metadata["native_stages"].items():
        path = out / name
        data = path.read_bytes()
        if len(data) != record["bytes"] or digest(data) != record["sha256"]:
            raise SystemExit(f"native stage hash mismatch: {path}")
        if decode(path) != (out / (name + ".iga.txt")).read_text():
            raise SystemExit(f"native stage disassembly drift: {path}")
    for label, key in (("vertex_TRUEOS_VS_state_v1.txt", "vs_state"),
                       ("fragment_TRUEOS_PS_state_v1.txt", "ps_state")):
        if state(out / label) != metadata[key]:
            raise SystemExit(f"native stage metadata drift: {label}")
        if push_ranges(out / label) != metadata[key.replace("state", "push_ranges")]:
            raise SystemExit(f"native Globals push layout drift: {label}")
    print("Verified exact skybox SPIR-V provenance, native hashes, state capture, and EU decode")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mesa-build", type=Path,
                        default=TRUEOS / ".codex_tmp/trueos-adj-instrumented-rpls/mesa-build")
    parser.add_argument("--work-dir", type=Path, default=VOXY / "target/skybox-native-bake")
    parser.add_argument("--out-dir", type=Path, default=VOXY / "shaderbin/native/skybox")
    parser.add_argument("--verify", action="store_true", help="verify the package without Mesa or GPU access")
    args = parser.parse_args()
    mesa = args.mesa_build.resolve()
    shim = mesa / "src/intel/tools/libintel_noop_drm_shim.so"
    driver = mesa / "src/intel/vulkan/libvulkan_intel.so"
    manifest = json.loads((VOXY / "shaderbin/manifest.json").read_text())
    stages = {}
    for name in ("skybox-vert", "skybox-frag"):
        item, = [s for s in manifest["shaders"] if s["profile"] == "trueos-bringup" and s["name"] == name]
        path = VOXY / "shaderbin" / item["file"]
        if digest(path.read_bytes()) != item["sha256"]:
            raise SystemExit(f"baked shader hash mismatch: {path}")
        stages[name] = (path, item)
    if args.verify:
        verify(args.out_dir.resolve(), stages)
        return
    if not shim.is_file() or not driver.is_file():
        raise SystemExit("instrumented ANV and mandatory no-op DRM shim required")
    baker = load_baker()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    capture = work / "intel"
    if capture.exists():
        shutil.rmtree(capture)
    capture.mkdir()
    source = work / "skybox_dump.c"
    source.write_text(compile_source(baker))
    program = work / "skybox_dump"
    baker.run(["cc", str(source), "-o", str(program), *baker.vulkan_compile_flags()])
    icd = work / "icd.json"
    icd.write_text(json.dumps({"file_format_version": "1.0.1", "ICD": {
        "api_version": "1.4.346", "library_path": str(driver)}}))
    env = os.environ.copy()
    env.update(LD_PRELOAD=str(shim), VK_DRIVER_FILES=str(icd), VK_ICD_FILENAMES=str(icd),
               INTEL_STUB_GPU_DEVICE_ID="4680", TRUEOS_VK_DEVICE_ID="0x4680",
               MESA_SHADER_CACHE_DISABLE="true", TRUEOS_EXECUTABLE_DUMP_DIR=str(capture))
    log = work / "compile.log"
    baker.run([str(program), *(str(p) for p, _ in stages.values())], env=env, log=log)
    device, executables = baker.parse_compile_log(log.read_text())
    if device["device_id"] != 0x4680 or "voxy_skybox: compiled_only=1" not in log.read_text():
        raise SystemExit("target-matched compile-only proof missing")
    vs_state = state(capture / "vertex_TRUEOS_VS_state_v1.txt")
    ps_state = state(capture / "fragment_TRUEOS_PS_state_v1.txt")
    vs_ranges = push_ranges(capture / "vertex_TRUEOS_VS_state_v1.txt")
    ps_ranges = push_ranges(capture / "fragment_TRUEOS_PS_state_v1.txt")
    if (vs_ranges[0]["start"], vs_ranges[0]["length"], ps_ranges[0]["start"], ps_ranges[0]["length"]) != (4, 3, 8, 1):
        raise SystemExit("skybox Globals payload layout changed; audit the native draw contract before rebaking")
    vs, ps8, ps16 = baker.extract_native(capture, work / "native")
    binaries = {"skybox.vs.simd8.bin": vs, "skybox.ps.simd8.bin": ps8}
    if ps16 is not None:
        binaries["skybox.ps.simd16.bin"] = ps16
    # Decode every complete stage before publishing any package.
    decoded = {}
    for name, data in binaries.items():
        path = work / name
        path.write_bytes(data)
        decoded[name] = decode(path)
    out = args.out_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    for name, data in binaries.items():
        (out / name).write_bytes(data)
        (out / (name + ".iga.txt")).write_text(decoded[name])
    for path in capture.glob("*state_v1.txt"):
        shutil.copy2(path, out / path.name)
    shutil.copy2(log, out / "compile.log")
    metadata = {
        "schema": "voxy-native-skybox-v1", "target": device,
        "source_profile": "trueos-bringup", "source_stages": [item for _, item in stages.values()],
        "new_native_compilation": True, "native_execution_admitted": False,
        "host_render_verified": False, "baremetal_render_verified": False,
        "vertex_stride": 12, "entry_points": ["main", "main"],
        "color_formats": ["Rgba16Float", "Rgba8Uint"], "depth_format": "Depth32Float",
        "depth_compare": "GreaterEqual", "depth_clear": 0, "sample_count": 1,
        "cull_mode": "Back", "front_face": "Ccw",
        "uniform": {"set": 0, "binding": 0, "layout": "Voxy Globals std140"},
        "composition": {"preserve_scene_alpha": True, "clouds_bareminimum_sets_alpha": 1,
                        "requires_composition_before_ui4_publication": True},
        "vs_state": vs_state, "ps_state": ps_state, "executables": executables,
        "vs_push_ranges": vs_ranges, "ps_push_ranges": ps_ranges,
        "native_stages": {name: {"bytes": len(data), "sha256": digest(data)} for name, data in binaries.items()},
        "presentation": {"target": "paired UI4 background", "buffering": "streaming/triple", "implemented": False},
    }
    (out / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    verify(out, stages)
    print(f"Captured exact bring-up skybox native stages in {out}; execution not yet admitted")


if __name__ == "__main__":
    sys.dont_write_bytecode = True
    main()
