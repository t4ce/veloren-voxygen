# Precompiled Voxygen shaders

The client always embeds and loads these SPIR-V artifacts. No runtime GLSL
compiler or shaderc dependency is included. WGPU still processes SPIR-V and
creates GPU pipelines.

Run from the repository root:

```
cargo run --manifest-path voxygen/Cargo.toml --no-default-features
```

The 82 artifacts cover 41 shader stages at the default render settings, with
shadow-map and cheap-shadow variants. Additional settings and experimental
shaders require additional baked variants. Source and binary hashes are
checked at runtime; unsupported or stale variants fail without a compiler
fallback. GLSL assets remain necessary for variant selection and verification.

Verify without a compiler:

```
python3 voxygen/tools/bake_shaders.py --verify
```

Rebuild offline with glslc installed:

```
python3 voxygen/tools/bake_shaders.py
```

The bake uses Vulkan 1.1, GLSL 430 core and performance optimization.
