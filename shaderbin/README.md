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

## TRUEOS bring-up baseline

`../trueos-bringup-profile.json` overlays the existing serialized settings schema
on every TRUEOS startup, after saved settings load. It is embedded in the build;
edit and rebuild to change the deployment contract. Settings remain mutable during
the session, and saved graphics values are overridden again on the next startup.
Window placement, credentials, controls and other unlisted settings are preserved.

Distances and frame caps use the graphics UI minima (terrain/entities 1, sprites
and figure detail 50, distant LoD 0, LoD detail 100, foreground/background 15/5 FPS).
Internal resolution is 0.1, AA is disabled, and optional effects are disabled,
including inverse effect-removal shader toggles. Brightness and field of view are
not workload minima and remain user settings.

The baker includes a `trueos-bringup` variant alongside the existing variants,
reading the inverse toggles from the same JSON. After changing pipeline settings,
update the bake mapping if needed and rebake. These SPIR-V artifacts do not grant
native TRUEOS GPU admission or complete the game-scene backend port.

## Native skybox slice

`bake_skybox_native.py` compiles the exact `trueos-bringup` skybox SPIR-V
pair through instrumented Mesa/ANV for ADL-S `8086:4680`. Its mandatory no-op
DRM shim allows compilation and capture without submitting GPU work. It keeps
the native stage binaries, IGA decodes, compiler state captures, hashes, and
provenance in `native/skybox`. Apply `mesa-skybox-push-capture.patch` to the
instrumented Mesa tree and rebuild ANV before baking; it captures the promoted
Globals uniform ranges rather than deriving them from assembly guesses.

```
python3 shaderbin/bake_skybox_native.py
python3 shaderbin/bake_skybox_native.py --verify
cargo test --manifest-path tests/scene-contract/Cargo.toml
```

The fixed contract is a 12-byte position vertex, one Globals UBO, no samplers,
one sample, back-face culling, and reverse `Depth32Float` depth. The capture uses
the renderer's preferred `Rgba16Float` scene colour and `Rgba8Uint` material
attachment. The VS promotes Globals bytes 128..224; the PS promotes bytes
256..288. `src/render/native_skybox.rs` preserves these ranges verbatim.

The skybox preserves destination alpha, so its scene colour still needs the
minimal composition path, which sets alpha to one, before presentation.
Runtime shader admission, composition execution, and submission to the paired
UI4 background are not implemented by this bake. Metadata deliberately records
native execution admission and host/bare-metal render verification as false.
The Iced character-selection foreground is not changed by this slice. Its
existing full-scene startup gate is also still in place.
