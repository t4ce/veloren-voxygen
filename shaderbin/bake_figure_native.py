#!/usr/bin/env python3
"""Compile the original bring-up figure pair to static target-matched Intel Gen12 ISA, without GPU execution."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import struct

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('skybox_bake', ROOT / 'shaderbin/bake_skybox_native.py')
sky = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sky)


def compile_source(baker):
    source = sky.compile_source(baker).replace('voxy_skybox: compiled_only=1', 'voxy_figure: compiled_only=1')
    source = baker.replace_once(source, '.stride = 12,', '.stride = 8,')
    source = baker.replace_once(source, 'VK_FORMAT_R16G16B16A16_SFLOAT', 'VK_FORMAT_R8G8B8A8_UNORM')
    start = source.index('    const VkVertexInputAttributeDescription attribute = {')
    end = source.index('    const VkPipelineInputAssemblyStateCreateInfo input_assembly', start)
    source = source[:start] + '''
    const VkVertexInputAttributeDescription attributes[2] = {
        { .location = 0, .binding = 0, .format = VK_FORMAT_R32_UINT, .offset = 0 },
        { .location = 1, .binding = 0, .format = VK_FORMAT_R32_UINT, .offset = 4 },
    };
    const VkPipelineVertexInputStateCreateInfo vertex_input = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
        .vertexBindingDescriptionCount = 1, .pVertexBindingDescriptions = &binding,
        .vertexAttributeDescriptionCount = 2, .pVertexAttributeDescriptions = attributes,
    };
''' + source[end:]
    source = baker.replace_once(source, '{ .blendEnable = VK_TRUE,', '{ .blendEnable = VK_FALSE,')
    start = source.index('    const VkPipelineLayoutCreateInfo pipeline_layout_info = {')
    end = source.index('    VkPipelineLayout pipeline_layout;', start)
    source = source[:start] + '''
    const VkDescriptorSetLayoutCreateInfo empty_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
    };
    VkDescriptorSetLayout empty_layout;
    CHECK_VK(vkCreateDescriptorSetLayout(device, &empty_info, NULL, &empty_layout));
    const VkDescriptorSetLayoutBinding atlas_bindings[2] = {
        { .binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 1, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
    };
    const VkDescriptorSetLayoutCreateInfo atlas_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 2, .pBindings = atlas_bindings,
    };
    VkDescriptorSetLayout atlas_layout;
    CHECK_VK(vkCreateDescriptorSetLayout(device, &atlas_info, NULL, &atlas_layout));
    const VkDescriptorSetLayoutBinding locals_bindings[2] = {
        { .binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT | VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 1, .descriptorType = VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
          .descriptorCount = 1, .stageFlags = VK_SHADER_STAGE_VERTEX_BIT | VK_SHADER_STAGE_FRAGMENT_BIT },
    };
    const VkDescriptorSetLayoutCreateInfo locals_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 2, .pBindings = locals_bindings,
    };
    VkDescriptorSetLayout locals_layout;
    CHECK_VK(vkCreateDescriptorSetLayout(device, &locals_info, NULL, &locals_layout));
    const VkDescriptorSetLayout set_layouts[4] = { globals_layout, empty_layout, atlas_layout, locals_layout };
    const VkPipelineLayoutCreateInfo pipeline_layout_info = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        .setLayoutCount = 4, .pSetLayouts = set_layouts,
    };
''' + source[end:]
    return source


def sha(data):
    return hashlib.sha256(data).hexdigest()


def ranges(path):
    return [dict((k, int(v)) for k, v in re.findall(r'(\w+)=(\d+)', line))
            for line in path.read_text().splitlines() if line.startswith('TRUEOS_PUSH_RANGE ')]


def sources():
    manifest = json.loads((ROOT / 'shaderbin/manifest.json').read_text())
    result = {}
    for name in ('figure-vert', 'figure-frag'):
        item, = [x for x in manifest['shaders'] if x['profile'] == 'trueos-bringup' and x['name'] == name]
        path = ROOT / 'shaderbin' / item['file']
        if sha(path.read_bytes()) != item['sha256']:
            raise SystemExit(f'stale original stage: {path}')
        result[name] = (path, item)
    return result


def package_digest(stages):
    value = 0xcbf29ce484222325
    for byte in b'voxy-figure-trueos-bringup-v1\0' + b''.join(path.read_bytes() for path, _ in stages.values()):
        value = ((value ^ byte) * 0x100000001b3) & 0xffffffffffffffff
    return value


def kernel_source(out, stages, equivalent=None):
    meta = json.loads((out / 'metadata.json').read_text())
    vendor, device, revision = meta['physical_targets'][0]
    targets = meta['physical_targets'].copy()
    if equivalent is not None:
        other = json.loads((equivalent / 'metadata.json').read_text())
        verify(equivalent, stages, other['target']['device_id'])
        for key in ('source_stages', 'native_stages', 'states', 'push_ranges', 'color_formats', 'depth_format', 'vertex_attributes'):
            if meta[key] != other[key]:
                raise SystemExit(f'equivalent package differs: {key}')
        targets += [target for target in other['physical_targets'] if target not in targets]
    targets.sort()

    vs = meta['states']['vertex']
    ps = meta['states']['fragment']
    source = f'// Generated by voxy/shaderbin/bake_figure_native.py for Intel {vendor:04X}:{device:04X} rev{revision:02X}.\n'
    source += '// Original bring-up SPIR-V; packed figure mesh, atlas and pose contract.\nuse super::*;\n'
    source += f'pub(crate) const PACKAGE_FNV1A64: u64 = 0x{package_digest(stages):016X};\n'
    source += f'pub(crate) const PHYSICAL_TARGETS: [(u16, u16, u8); {len(targets)}] = [\n'
    source += ''.join(f'    (0x{v:04X}, 0x{d:04X}, 0x{r:02X}),\n' for v, d, r in targets) + '];\n'
    source += 'pub(crate) fn supports(vendor: u16, device: u16, revision: u8) -> bool {\n    PHYSICAL_TARGETS.contains(&(vendor, device, revision))\n}\n'
    lengths = {}
    for name, filename in [('VS', 'figure.vs.simd8.bin'), ('PS', 'figure.ps.simd16.bin')]:
        data = (out / filename).read_bytes()
        words = struct.unpack(f'<{len(data)//4}I', data)
        lengths[name] = len(data)
        source += f'// SHA256 {sha(data)}\nstatic {name}_CODE: [u32; {len(words)}] = [\n'
        source += ''.join('    ' + ', '.join(f'0x{w:08X}' for w in words[i:i+8]) + ',\n' for i in range(0, len(words), 8))
        source += '];\n'
    for stage in ('vertex', 'fragment'):
        records = [r for r in meta['push_ranges'][stage] if r['length']]
        source += f'pub(crate) const {stage.upper()}_PUSH_RANGES: [(u32, u32, u32, u32); {len(records)}] = [\n'
        source += ''.join(f"    ({r['set']}, {r['index']}, {r['start'] * 32}, {r['length'] * 32}),\n" for r in records)
        source += '];\n'
    source += f"""pub(crate) static PIPELINE: TrianglePipeline = TrianglePipeline {{
    vs: TriangleVertexShader {{ code: &VS_CODE, meta: TriangleVertexShaderMetadata {{
        kernel: ShaderKernelMetadata {{ code_offset_bytes: 0, code_size_bytes: {lengths['VS']},
            code_alignment_bytes: 64, ksp_offset_bytes: 0, dispatch_mode: DispatchMode::Simd8,
            grf_start_register: {vs['dispatch_grf_start']}, grf_used: 128,
            push_constant_bytes: {sum(r['length'] * 32 for r in meta['push_ranges']['vertex'])},
            binding_table_entry_count: {vs['binding_table_entries']}, sampler_count: {vs['sampler_count']} }},
        max_threads: {vs['max_threads']}, urb_entry_output_length: {vs['urb_entry_64b']} }} }},
    ps: TrianglePixelShader {{ code: &PS_CODE, meta: TrianglePixelShaderMetadata {{
        kernel: ShaderKernelMetadata {{ code_offset_bytes: {(lengths['VS'] + 63) & ~63}, code_size_bytes: {lengths['PS']},
            code_alignment_bytes: 64, ksp_offset_bytes: 0, dispatch_mode: DispatchMode::Simd16,
            grf_start_register: {ps['grf_start16']}, grf_used: 128,
            push_constant_bytes: {ps['push_bytes']}, binding_table_entry_count: {ps['binding_table_entries']},
            sampler_count: {ps['sampler_count']} }},
        num_varying_inputs: {ps['num_varying_inputs']}, uses_vmask: {str(bool(ps['uses_vmask'])).lower()},
        computed_stencil: {str(bool(ps['computed_stencil'])).lower()}, persample_dispatch: false,
        computed_depth_mode: {ps['computed_depth_mode']}, flat_inputs: {ps['flat_inputs']} }} }},
}};
"""
    return source


def verify(out, stages, device_id):
    meta = json.loads((out / 'metadata.json').read_text())
    assert meta['source_stages'] == [item for _, item in stages.values()]
    assert meta['native_execution_admitted'] is False
    assert meta['target']['device_id'] == device_id
    for name, artifact in meta['native_stages'].items():
        data = (out / name).read_bytes()
        assert sha(data) == artifact['sha256'] and len(data) == artifact['bytes']
        assert sky.decode(out / name) == (out / (name + '.iga.txt')).read_text()
    for name, expected in meta['capture_sha256'].items():
        assert sha((out / name).read_bytes()) == expected
    for stage in ('vertex', 'fragment'):
        path = out / f'{stage}_TRUEOS_{"VS" if stage == "vertex" else "PS"}_state_v1.txt'
        assert sha(path.read_bytes()) == meta['state_sha256'][stage]
        assert ranges(path) == meta['push_ranges'][stage]
    print('Verified original figure SPIR-V, native ISA, state and uniform ranges; execution not admitted')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mesa-build', type=Path, default=sky.TRUEOS / '.codex_tmp/trueos-adj-instrumented-rpls/mesa-build')
    parser.add_argument('--work-dir', type=Path, default=ROOT / 'target/figure-native-bake')
    parser.add_argument('--out-dir', type=Path, default=ROOT / 'shaderbin/native/figure/adls')
    parser.add_argument('--device-id', type=lambda value: int(value, 0), default=0x4680, choices=[0x9A49, 0x4680])
    parser.add_argument('--equivalent-package', type=Path, help='also admit this independently verified identical bake')
    parser.add_argument('--kernel-output', type=Path, help='emit the sealed target-matched kernel Rust package')
    parser.add_argument('--verify', action='store_true')
    args = parser.parse_args()
    stages = sources()
    out = args.out_dir.resolve()
    if args.verify:
        verify(out, stages, args.device_id)
        if args.kernel_output:
            if args.kernel_output.read_text() != kernel_source(out, stages, args.equivalent_package):
                raise SystemExit('stale kernel figure package')
        return
    mesa = args.mesa_build.resolve()
    shim = mesa / 'src/intel/tools/libintel_noop_drm_shim.so'
    driver = mesa / 'src/intel/vulkan/libvulkan_intel.so'
    if not shim.is_file() or not driver.is_file():
        raise SystemExit('instrumented ANV and mandatory no-op DRM shim required')
    baker = sky.load_baker()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    capture = work / 'intel'
    if capture.exists():
        shutil.rmtree(capture)
    capture.mkdir()
    source = work / 'figure_dump.c'
    source.write_text(compile_source(baker))
    program = work / 'figure_dump'
    baker.run(['cc', str(source), '-o', str(program), *baker.vulkan_compile_flags()])
    icd = work / 'icd.json'
    icd.write_text(json.dumps({'file_format_version': '1.0.1', 'ICD': {'api_version': '1.4.346', 'library_path': str(driver)}}))
    env = os.environ.copy()
    env.update(LD_PRELOAD=str(shim), VK_DRIVER_FILES=str(icd), VK_ICD_FILENAMES=str(icd),
               INTEL_STUB_GPU_DEVICE_ID=f'{args.device_id:04x}', TRUEOS_VK_DEVICE_ID=f'0x{args.device_id:04x}',
               MESA_SHADER_CACHE_DISABLE='true', TRUEOS_EXECUTABLE_DUMP_DIR=str(capture))
    log = work / 'compile.log'
    baker.run([str(program), *(str(p) for p, _ in stages.values())], env=env, log=log)
    device, executables = baker.parse_compile_log(log.read_text())
    if device['device_id'] != args.device_id or 'voxy_figure: compiled_only=1' not in log.read_text():
        raise SystemExit('target-matched compile-only proof missing')
    # Use the instrumented serializer's explicit stage/size record. Scanning
    # the opaque pipeline cache can mistake reflection data for instructions.
    def serialized(stage, name):
        records = list(capture.glob(name))
        if len(records) != 1:
            raise SystemExit(f'expected one serialized stage {stage}')
        data = records[0].read_bytes()
        actual_stage, size = struct.unpack_from('<II', data)
        if actual_stage != stage or size != len(data) - 8:
            raise SystemExit(f'invalid serialized stage header: {records[0]}')
        return data[8:]
    vs = serialized(0, '*vertex*shader_serialize.bin')
    vs_size = baker.assembly_code_size(next(capture.glob('*vertex*GEN_Assembly.txt')))
    # The serializer contains a trailing compact NOP after the executable
    # assembly span. Validate its exact bytes instead of decoding it as ISA.
    trailer = bytes.fromhex('6000002000000000')
    if not 0 < vs_size <= len(vs) or vs[vs_size:] != trailer:
        raise SystemExit('VS span does not match serialized code and captured trailer')
    vs = vs[:vs_size]
    combined = serialized(4, '*fragment*shader_serialize.bin')
    ps_state = sky.state(capture / 'fragment_TRUEOS_PS_state_v1.txt')
    offset16 = ps_state['offset16']
    fragments = sorted(capture.glob('*fragment*GEN_Assembly.txt'))
    ps8_size = baker.assembly_code_size(fragments[0])
    ps16_size = baker.assembly_code_size(fragments[1]) if ps_state['dispatch16'] else 0
    if ps_state['dispatch16']:
        if offset16 % 64 or ps8_size > offset16 or offset16 + ps16_size > len(combined):
            raise SystemExit('invalid fragment executable spans')
        padding = combined[ps8_size:offset16]
        ps_trailer = combined[offset16 + ps16_size:]
    else:
        if ps8_size > len(combined):
            raise SystemExit('invalid fragment executable span')
        padding = b''
        ps_trailer = combined[ps8_size:]
    if any(padding) or ps_trailer != trailer:
        raise SystemExit('nonzero data outside fragment executable spans')
    ps8 = combined[:ps8_size]
    ps16 = combined[offset16:offset16 + ps16_size] if ps_state['dispatch16'] else None
    binaries = {'figure.vs.simd8.bin': vs, 'figure.ps.simd8.bin': ps8}
    if ps16 is not None:
        binaries['figure.ps.simd16.bin'] = ps16
    decoded = {}
    for name, data in binaries.items():
        path = work / name
        path.write_bytes(data)
        decoded[name] = sky.decode(path)
    out.mkdir(parents=True, exist_ok=True)
    for name, data in binaries.items():
        (out / name).write_bytes(data)
        (out / (name + '.iga.txt')).write_text(decoded[name])
    for path in capture.glob('*state_v1.txt'):
        shutil.copy2(path, out / path.name)
    capture_files = sorted(capture.glob('*Final_NIR.txt')) + sorted(capture.glob('*Shader_push_map.txt'))
    for path in capture_files:
        shutil.copy2(path, out / path.name)
    shutil.copy2(log, out / 'compile.log')
    state_paths = {stage: out / f'{stage}_TRUEOS_{suffix}_state_v1.txt' for stage, suffix in [('vertex', 'VS'), ('fragment', 'PS')]}
    meta = {
        'schema': 'voxy-native-figure-v1', 'target': device,
        'physical_targets': [[0x8086, args.device_id, 0x01 if args.device_id == 0x9A49 else 0x0C]],
        'source_profile': 'trueos-bringup', 'source_stages': [item for _, item in stages.values()],
        'new_native_compilation': True, 'native_execution_admitted': False,
        'host_render_verified': False, 'baremetal_render_verified': False,
        'vertex_stride': 8, 'vertex_attributes': ['Uint32', 'Uint32'],
        'color_formats': ['Rgba8Unorm', 'Rgba8Uint'], 'depth_format': 'Depth32Float',
        'depth_compare': 'GreaterEqual', 'depth_clear': 0, 'sample_count': 1,
        'cull_mode': 'Back', 'front_face': 'Ccw', 'blend': False,
        'uniforms': [{'set': 0, 'binding': 0, 'layout': 'Voxy Globals std140'},
                     {'set': 3, 'binding': 0, 'bytes': 144}, {'set': 3, 'binding': 1, 'bytes': 2048}],
        'atlas': {'set': 2, 'texture_binding': 0, 'sampler_binding': 1, 'format': 'Rgba8Unorm'},
        'states': {stage: sky.state(path) for stage, path in state_paths.items()},
        'push_ranges': {stage: ranges(path) for stage, path in state_paths.items()},
        'state_sha256': {stage: sha(path.read_bytes()) for stage, path in state_paths.items()},
        'executables': executables,
        'capture_sha256': {path.name: sha(path.read_bytes()) for path in capture_files},
        'extraction': {'source': 'instrumented ANV shader serializer',
                       'header': 'little-endian stage:u32, program_size:u32',
                       'vs_executable_bytes': vs_size, 'ps8_executable_bytes': ps8_size,
                       'ps16_offset': offset16, 'ps16_executable_bytes': ps16_size,
                       'inter_executable_padding': 'checked zero, excluded',
                       'serialized_trailer_hex': trailer.hex(),
                       'serialized_trailer': 'checked exact, excluded from executable stages'},
        'native_stages': {name: {'bytes': len(data), 'sha256': sha(data)} for name, data in binaries.items()},
    }
    (out / 'metadata.json').write_text(json.dumps(meta, indent=2) + '\n')
    verify(out, stages, args.device_id)
    if args.kernel_output:
        args.kernel_output.write_text(kernel_source(out, stages, args.equivalent_package))


if __name__ == '__main__':
    main()
