# Precompiled Voxygen shaders

The client always embeds and loads these SPIR-V artifacts. No runtime GLSL
compiler or shaderc dependency is included. WGPU still processes SPIR-V and
creates GPU pipelines.

Run from the repository root:

```
cargo run
```

The 126 artifacts cover 41 shader stages for the Minimal preset and the original
default settings with shadow-map and cheap-shadow variants. New userdata uses
the Minimal preset: FX upscaling, flat clouds, low fluid/reflections, Lambertian
lighting, no shadows, no bloom and no point glow. Additional settings and experimental
shaders require additional baked variants. Source and binary hashes are
checked at runtime; unsupported or stale variants fail without a compiler
fallback. GLSL assets remain necessary for variant selection and verification.

Verify without a compiler:

```
python3 shaderbin/bake_shaders.py --verify
```

Rebuild offline with glslc installed:

```
python3 shaderbin/bake_shaders.py
```

The bake uses Vulkan 1.1, GLSL 430 core and performance optimization.

TRUEOS selects a display-color postprocess variant: gamma (including the
existing 0.3 exponent offset), scene fade, and the low-quality underwater RGB
tint run through the primary display LUT. Exposure, AA/upscaling, bloom
composition and dithering stay in the shader. Software variants retain the
original operations. The postprocess source is embedded from
`shaderbin/postprocess-frag.glsl`, so installing the packed app does not require
an asset-database refresh. The bake also accepts the sibling `voxy-assets` tree.

The CPU ramp accounts for the selected surface's sRGB encoding and is uploaded
only when gamma, fade, tint or encoding changes. Portal display fades compose
over it, and the kernel restores the previous display state at window close or
VM teardown. These transforms affect the HUD and other content on Pipe A; game
screenshots capture pixels before the display LUT. Use the updated kernel
together with this app for fade composition and restoration. No additional
graphics controls or render passes are introduced.
