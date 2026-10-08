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
native foreground transport is described below; this baked native pair is
not used by that transport.

## Fixed skybox draw and host reference

`src/render/skybox_feature.rs` draws only the original skybox mesh and the
shipped bring-up shaders. It retains `Rgba16Float` colour, `Rgba8Uint` material,
and reverse `Depth32Float` attachments. The original minimal postprocess copies
HDR RGB to the display target and makes its alpha opaque. Under `BareMinimum`,
the intervening clouds pass performs the same copy, so this slice omits it.
The draw node accepts an existing device and output view; it neither admits a
TRUEOS execution device nor acquires or publishes a UI4 lease.

The standalone executable includes this draw node and the production skybox
mesh generator. It runs the actual baked SPIR-V, checks every pixel for day,
dusk, and night, verifies that dusk's 1.78 red survives the HDR intermediate,
and checks opaque display output at twice the internal resolution. It requests
no optional device features. Run the software Vulkan reference explicitly:

```
VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json \
  cargo run --locked --manifest-path tests/skybox-render/Cargo.toml
```

This is a host render check, independent of the native Intel bake above. It
does not establish native shader admission, GPU completion, or publication on
the paired UI4 triple-buffered background.

The same executable also tests `tests/skybox-render/src/direct_rgba8.rs`: the
unchanged bring-up skybox SPIR-V and original mesh render directly into one
`Rgba8Unorm` target, with replacement alpha, no material/depth attachments, and
no postprocess. It compares every output byte with the HDR/copy reference for
day, dusk, night, and two intermediate sun directions (tolerance one 8-bit
step, allowing the removed FP16 rounding). This simplification applies only
to the fixed `BareMinimum` sky-only feature: its fragment colour is uniform
and its postprocess does not tone-map. General scene HDR must be evaluated
separately. This experiment still establishes no native TRUEOS execution.

## Post-login native minimal sky

TRUEOS login now starts the bounded `minimal_sky::NativeSky` producer instead
of constructing all game pipelines. The scene worker opens a vgpu render
device/queue, imports the existing paired background's RGBA8 write lease,
submits the GPU-backed full-target clear, and publishes its exact release
through UI4. Login waits for that first publication. Loader foreground work
cannot replace the sky background. The existing paired worker keeps the
triple-buffering, Busy retries, resize, and logout handoff contracts.

`minimal_sky::rgba8` evaluates the shipped `BareMinimum` + Flat-cloud sky
colour from the connected client's sun direction; it matches the original
baked shaders' opaque display output in the host render proof. This uniform
sky uses the kernel's shipped AOT fill implementation, not the native skybox
VS/PS package. Directional sky features remain outside this fixed profile.

The original character-selection and server-rules Iced widgets use the native
BCS0 foreground planner/presenter, without constructing figures or LoD scenes.
Character list operations remain active; world entry and spectating report
that world rendering is unavailable in this sky-only build. The full scene
adapter remains unimplemented. Build validation and host tests do not establish
bare-metal presentation; verify the new Blueprint on the native rig separately.


## Admitted native Flat cloud layer

The native renderer now draws the existing Flat cloud plane as a transparent,
premultiplied RGBA8 layer before terrain. Terrain loads that color and clears
only its depth. ICED remains in the foreground. The display helper supplies the
sky backdrop; the cloud shader applies no gamma. Avatar and Conrod remain guarded.
Character selection draws clouds and sky; world entry adds synchronized terrain.

The sealed cloud package targets Intel ADL-S `8086:4680`, revision `0C`. Its
original noise image, exact floating-point weather coverage, world altitude,
time and camera inputs share one packed sampled image. Coverage and placement
remain controlled by server weather. Clear weather correctly produces no clouds.
Invalid world/camera data leaves the cloud layer transparent until synchronization.

Bake with `python3 shaderbin/bake_flat_cloud_native.py`; verify shipped sources,
ISA and both SDK package constants with `--verify`. The bake uses the compile-only
Intel driver shim and captures native fixed-function shader metadata. It does not
execute on the physical GPU. `tests/flat-clouds` compares the packed SPIR-V against
the existing Flat shader on host Vulkan, allowing hardware filtering quantization.
`tests/native-terrain/run.py` checks native cloud transport ownership and terrain
color preservation. Deploy both the rebuilt kernel and `voxy.bp`: blueprint-only
redeployment cannot add a kernel shader admission. Physical presentation still
requires testing on the native rig.


Native cloud noise runs at half game-time speed, sampled at the existing frame
cadence (foreground cap 30 FPS). Sun direction, camera and server weather retain
their live inputs. The kernel emits `voxy-clouds: phase=timing` at Important level
every 128 successful cloud submissions, with target extent, average phase times,
average total time and maximum render/total time. GPU polling is included in
render time. These are submission wall times, excluding blueprint-side packet
construction/upload and UI4 publication, rather than hardware GPU timestamps.
