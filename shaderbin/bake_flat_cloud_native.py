#!/usr/bin/env python3
"""Bake Voxy Flat clouds for ADL-S. Single sampled input, no shader gamma.
The packed RGBA8 input carries exact f32 frame/weather values and the original
noise pixels. This fits the existing typed native sampled-draw capability.
"""
import hashlib, importlib.util, json, os, re, shutil, struct, subprocess, sys
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
OS = ROOT.parent / "TRUEOS"
ASSETS = ROOT.parent / "voxy-assets/voxygen/shaders"
OUT = ROOT / "shaderbin/native/clouds"
WORK = ROOT / "target/flat-cloud-native"
spec = importlib.util.spec_from_file_location("baker", OS / "tools/helio-intel-bake/bake.py")
baker = importlib.util.module_from_spec(spec); spec.loader.exec_module(baker)
def expand(text):
    def inc(m):
        name = m[1]
        path = ROOT/"shaderbin/cloud-flat.glsl" if name=="cloud.glsl" else ASSETS/"include"/name
        data = path.read_text()
        if name=="constants.glsl":
            data += "\n#define CLOUD_MODE CLOUD_MODE_FLAT\n#define SHADOW_MODE SHADOW_MODE_NONE\n#define FLUID_MODE FLUID_MODE_LOW\n#define LIGHTING_ALGORITHM LIGHTING_ALGORITHM_LAMBERTIAN\n"
        if name in ("globals.glsl", "rain_occlusion.glsl"):
            data = re.sub(r"layout\s*\([^)]*\)\s*uniform\s+\w+\s*\{(.*?)\};", r"\1", data, flags=re.S)
        return expand(data)
    return re.sub(r"(?m)^#include +<(.+)>\s*$",inc,text)
prefix = """#version 450
layout(set=0,binding=3) uniform texture2D packed_input;
layout(set=0,binding=4) uniform sampler packed_sampler;
float scalar_at(ivec2 p) {
    uvec4 b = uvec4(round(texelFetch(sampler2D(packed_input,packed_sampler),p,0)*255.0));
    return uintBitsToFloat(b.r | (b.g<<8) | (b.b<<16) | (b.a<<24));
}
float param(int i) { return scalar_at(ivec2(i,0)); }
float noise_sample(vec2 uv) {
    ivec2 size = ivec2(param(28),param(29));
    vec2 p = fract(uv)*vec2(size)-0.5;
    ivec2 base = ivec2(floor(p)); vec2 f=fract(p);
    ivec2 a = (base % size + size) % size;
    ivec2 b = ((base+1) % size + size) % size;
    return mix(mix(texelFetch(sampler2D(packed_input,packed_sampler),ivec2(a.x,a.y+1),0).r,
                   texelFetch(sampler2D(packed_input,packed_sampler),ivec2(b.x,a.y+1),0).r,f.x),
               mix(texelFetch(sampler2D(packed_input,packed_sampler),ivec2(a.x,b.y+1),0).r,
                   texelFetch(sampler2D(packed_input,packed_sampler),ivec2(b.x,b.y+1),0).r,f.x),f.y);
}
vec4 weather_sample(vec2 uv) {
    ivec2 size = ivec2(param(30),param(31));
    vec2 p=uv*vec2(size)-0.5; ivec2 base=ivec2(floor(p)); vec2 f=fract(p);
    ivec2 a=clamp(base,ivec2(0),size-1), b=clamp(base+1,ivec2(0),size-1);
    int y=1+int(param(29));
    float v=mix(mix(scalar_at(ivec2(a.x,a.y+y)),scalar_at(ivec2(b.x,a.y+y)),f.x),
                mix(scalar_at(ivec2(a.x,b.y+y)),scalar_at(ivec2(b.x,b.y+y)),f.x),f.y);
    return vec4(v,0,0,0);
}
"""
source = expand("#include <constants.glsl>\n#include <globals.glsl>\n#include <cloud.glsl>\n")
# Keep all original arithmetic, replacing resource access only.
source = re.sub(r"layout\s*\([^)]*\)\s*uniform\s+(?:texture\w+|sampler\w*)\s+(\w+);",
                lambda m: ("#define "+m[1]+" "+("packed_sampler" if m[1].startswith("s_") else "packed_input")+"\n"), source)
source = re.sub(r"textureLod\(sampler2D\(t_noise, s_noise\), ([^\n;]+?), 0\.0\)\.x", r"noise_sample(\1)", source)
source = re.sub(r"vec2 wpos_to_uv\(vec2 wpos\) \{.*?\n\}", """vec2 wpos_to_uv(vec2 wpos) { return (wpos+16.0)/(32.0*vec2(param(24),param(25))); }""", source, flags=re.S)
source = re.sub(r"vec4 sample_weather\(vec2 wpos\) \{.*?\n\}", "vec4 sample_weather(vec2 wpos) { return weather_sample(wpos_to_uv(wpos)); }", source, flags=re.S)
source += """
layout(location=0) in vec2 texture_uv;
layout(location=0) out vec4 color;
void main() {
    cam_pos=vec4(param(0),param(1),param(2),0);
    focus_off=vec4(0);
    view_distance=vec4(0,0,param(22),param(23));
    time_of_day=vec4(param(17),param(18),0,0);
    sun_dir=vec4(param(19),param(20),param(21),0);
    moon_dir=-sun_dir;
    ambiance=param(27);
    rain_density=param(26);
    vec3 right=vec3(param(4),param(5),param(6));
    vec3 up=vec3(param(8),param(9),param(10));
    vec3 forward=vec3(param(12),param(13),param(14));
    vec2 clip=texture_uv*2.0-1.0;
    vec3 ray=normalize(forward+right*clip.x*param(16)/param(15)+up*clip.y/param(15));
    vec4 cloud=get_flat_cloud_layer(ray,cam_pos.xyz,524288.0);
    color=vec4(clamp(cloud.rgb,vec3(0),vec3(cloud.a)),cloud.a);
}
"""
if "--verify" in sys.argv:
    meta=json.loads((OUT/"metadata.json").read_text())
    assert meta["physical_target"] == dict(vendor=0x8086,device=0x4680,revision=0x0c)
    assert meta["source_sha256"] == hashlib.sha256((prefix+source).encode()).hexdigest()
    assert meta["reference_source_sha256"] == hashlib.sha256((ROOT/"shaderbin/cloud-flat.glsl").read_bytes()).hexdigest()
    assert meta["fragment_sha256"] == hashlib.sha256((OUT/"clouds.ps.simd16.bin").read_bytes()).hexdigest()
    digest=0xcbf29ce484222325
    for b in b"voxy-flat-cloud-v1\0"+(OUT/"clouds.vert.spv").read_bytes()+(OUT/"clouds.frag.spv").read_bytes():
        digest=((digest^b)*0x100000001b3)&0xffffffffffffffff
    assert meta["package"] == f"0x{digest:016X}"
    generated=(OS/"crates/trueos-shader/generated_voxy_flat_cloud.rs").read_text()
    assert f"0x{digest:016X}" in generated
    for stage,artifact in [("VS","clouds.vs.simd8.bin"),("PS","clouds.ps.simd16.bin")]:
        body=re.search(r"static "+stage+r":.*?= \[(.*?)\];",generated,re.S)[1]
        words=[int(word,16) for word in re.findall(r"0x[0-9A-F]+",body)]
        assert struct.pack("<"+"I"*len(words),*words)==(OUT/artifact).read_bytes()
    for sdk in [OS/"crates/trueos-v/src/vgpu.rs",ROOT.parent/"TRUEOS-Blueprints/crates/trueos-v/src/vgpu.rs"]:
        match=re.search(r"SHADER_PACKAGE_VOXY_FLAT_CLOUD_FNV1A64: u64 = (0x[0-9A-Fa-f_]+)",sdk.read_text())
        assert int(match[1].replace("_",""),16)==digest
    print("Verified native Flat cloud sources, package, kernel ISA and SDK contract: "+meta["package"])
    sys.exit(0)
OUT.mkdir(parents=True,exist_ok=True); WORK.mkdir(parents=True,exist_ok=True)
(OUT/"clouds.frag").write_text(prefix+source)
vs_source=OS/"tools/clip-position3-uv-bake/shaders/clip_position3_uv.vert"
vs=WORK/"clouds.vert.spv"; ps=WORK/"clouds.frag.spv"
for path, glsl in [(vs,vs_source),(ps,OUT/"clouds.frag")]:
    subprocess.run(["glslc","--target-env=vulkan1.1","-O",str(glsl),"-o",str(path)],check=True)
dumper_src=WORK/"dump.c"; baker.make_churn_compile_only_dumper(dumper_src)
s=dumper_src.read_text().replace('.pName = "vs_main",','.pName = "main",').replace('.pName = "fs_main",','.pName = "main",')
s=s.replace(".stride = 24,", ".stride = 20,").replace("VK_FORMAT_R32G32B32_SFLOAT, .offset = 12","VK_FORMAT_R32G32_SFLOAT, .offset = 12")
s=s.replace("storage_bindings[3]","storage_bindings[5]").replace(".bindingCount = 3,", ".bindingCount = 5,")
needle='{ .binding = 2, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,\n          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },'
assert needle in s
s=s.replace(needle,needle+"""
        { .binding = 3, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 4, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
""")
dumper_src.write_text(s); dumper=WORK/"dump"
subprocess.run(["cc",str(dumper_src),"-o",str(dumper),*baker.vulkan_compile_flags()],check=True)
mesa=OS/".codex_tmp/trueos-adj-instrumented-rpls/mesa-build"
shim=mesa/"src/intel/tools/libintel_noop_drm_shim.so"; driver=mesa/"src/intel/vulkan/libvulkan_intel.so"
assert shim.is_file() and driver.is_file()
icd=WORK/"icd.json"; icd.write_text(json.dumps({"file_format_version":"1.0.1","ICD":{"api_version":"1.4.346","library_path":str(driver)}}))
capture=WORK/"intel"; shutil.rmtree(capture,ignore_errors=True); capture.mkdir()
env=os.environ.copy(); env.update(LD_PRELOAD=str(shim),VK_DRIVER_FILES=str(icd),VK_ICD_FILENAMES=str(icd),INTEL_STUB_GPU_DEVICE_ID="4680",TRUEOS_VK_DEVICE_ID="0x4680",MESA_SHADER_CACHE_DISABLE="true",TRUEOS_EXECUTABLE_DUMP_DIR=str(capture))
log=OUT/"compile.log"; baker.run([str(dumper),str(vs),str(ps)],env=env,log=log)
device,executable=baker.parse_compile_log(log.read_text()); assert device["device_id"]==0x4680
vcode,p8,pcode=baker.extract_native(capture,WORK/"native")
def state(name):
    p=capture/name; shutil.copy2(p,OUT/name)
    return {k:int(v,0) for k,v in re.findall(r"(\w+)=(0x[0-9a-fA-F]+|\d+)",p.read_text().splitlines()[0])}
vstate=state("vertex_TRUEOS_VS_state_v1.txt"); pstate=state("fragment_TRUEOS_PS_state_v1.txt")
assert vstate["uses_vertexid"]==0 and vstate["binding_table_entries"]==0
assert pstate["binding_table_entries"]==3 and pstate["sampler_count"]==0 and pstate["push_bytes"]==0
reference=(OS/"crates/trueos-shader/clip_position3_uv_texture/vertex_TRUEOS_VS_state_v1.txt").read_text()
reference_state={k:int(v,0) for k,v in re.findall(r"(\w+)=(0x[0-9a-fA-F]+|\d+)", reference.splitlines()[0])}
assert all(vstate[k]==v for k,v in reference_state.items())
(OUT/"clouds.vs.simd8.bin").write_bytes(vcode)
digest=0xcbf29ce484222325
for b in b"voxy-flat-cloud-v1\0"+vs.read_bytes()+ps.read_bytes(): digest=((digest^b)*0x100000001b3)&0xffffffffffffffff
(OUT/"clouds.ps.simd16.bin").write_bytes(pcode)
decoded=subprocess.check_output(["iga64","-d","-p=12p1","-Xprint-pc",str(OUT/"clouds.ps.simd16.bin")],text=True)
assert "illegal" not in decoded.lower(); (OUT/"clouds.ps.simd16.bin.iga.txt").write_text(decoded)
words=struct.unpack("<"+"I"*(len(pcode)//4),pcode)
rust="// Generated by voxy/shaderbin/bake_flat_cloud_native.py. ADL-S 8086:4680 rev0C.\nuse super::*;\n"
rust+=f"pub(crate) const PACKAGE: u64 = 0x{digest:016X};\nstatic PS: [u32; {len(words)}] = [\n"
rust+="".join("    "+", ".join(f"0x{x:08X}" for x in words[i:i+8])+",\n" for i in range(0,len(words),8))+"];\n"
vwords=struct.unpack("<"+"I"*(len(vcode)//4),vcode)
rust+=f"static VS: [u32; {len(vwords)}] = [\n"
rust+="".join("    "+", ".join(f"0x{x:08X}" for x in vwords[i:i+8])+",\n" for i in range(0,len(vwords),8))+"];\n"
rust+=f"""pub(crate) static PIPELINE: TrianglePipeline = TrianglePipeline {{
    vs: TriangleVertexShader {{ code: &VS, meta: TriangleVertexShaderMetadata {{
        kernel: ShaderKernelMetadata {{ code_offset_bytes: 0, code_size_bytes: {len(vcode)},
            code_alignment_bytes: 64, ksp_offset_bytes: 0, dispatch_mode: DispatchMode::Simd8,
            grf_start_register: 2, grf_used: 128, push_constant_bytes: 0,
            binding_table_entry_count: 0, sampler_count: 0 }},
        max_threads: {vstate["max_threads"]}, urb_entry_output_length: 1,
    }} }},
    ps: TrianglePixelShader {{ code: &PS, meta: TrianglePixelShaderMetadata {{
        kernel: ShaderKernelMetadata {{ code_offset_bytes: {(len(vcode)+63)&~63}, code_size_bytes: {len(pcode)},
            code_alignment_bytes: 64, ksp_offset_bytes: 0, dispatch_mode: DispatchMode::Simd16,
            grf_start_register: {pstate['grf_start16']}, grf_used: 128, push_constant_bytes: 0,
            binding_table_entry_count: 3, sampler_count: 0 }},
        num_varying_inputs: {pstate['num_varying_inputs']}, uses_vmask: {str(bool(pstate['uses_vmask'])).lower()},
        computed_stencil: false, persample_dispatch: false, computed_depth_mode: 0, flat_inputs: 0,
    }} }},
}};
"""
(OS/"crates/trueos-shader/generated_voxy_flat_cloud.rs").write_text(rust)
meta=dict(package=f"0x{digest:016X}",physical_target=dict(vendor=0x8086,device=0x4680,revision=0x0c),
    vertex_state=vstate,fragment_state=pstate, hardware_execution_verified=False,
    source_sha256=hashlib.sha256((prefix+source).encode()).hexdigest(),
    fragment_sha256=hashlib.sha256(pcode).hexdigest(),fragment_bytes=len(pcode),
    reference_source_sha256=hashlib.sha256((ROOT/"shaderbin/cloud-flat.glsl").read_bytes()).hexdigest())
(OUT/"metadata.json").write_text(json.dumps(meta,indent=2)+"\n")
shutil.copy2(ps,OUT/"clouds.frag.spv")
shutil.copy2(vs,OUT/"clouds.vert.spv")
print("Baked Flat clouds: native ADL-S package",hex(digest),"fragment",len(pcode),"bytes")
