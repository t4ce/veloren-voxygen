# TRUEOS scene bring-up: active passes and gates

The normal `run.rs` play-state loop remains the entry point. Main menu, server
rules, and the temporary existing-character selector use native Iced/UI4.
Joining still goes through the normal character request and `SessionState`.
No automatic login or character creation was introduced.

The TRUEOS scene adapter is a **CPU command/resource bridge**, not a full WGPU
GPU implementation. It retains source buffers and binding ranges, tracks
pipeline handles, and extracts the existing terrain draws. Texture descriptors
are retained, but texture contents and original shader execution are out-gated.
Unknown pipeline labels fail loudly rather than silently disappearing.

## Original minimum-profile pass map

These are source-level submissions, not measurements of actual GPU dispatch.
The original scene uses vertex/fragment render pipelines; no compute or
hardware tessellation dispatch occurs in this route.

| Pass / shader stems | Original minimum profile | Current TRUEOS scene bridge |
| --- | --- | --- |
| Directed terrain/figure/debug and point `shadow-*` | Off: ShadowMode::None | Out-gated |
| `rain-occlusion-*` terrain/figure | Off: rain_enabled=false | Out-gated |
| `terrain-*` opaque near chunks | Drawn after terrain streaming/meshing | Packed mesh quads become native GPU lines |
| `figure-*` viewpoint and other figures | Drawn if figure models exist | Out-gated |
| `lod-terrain-*` | Still submitted even with LoD distance zero | Out-gated |
| `skybox-*` | Still submitted; BareMinimum simplifies fragment color | Out-gated |
| `sprite-*` | Near sprites still submitted, distance 50 | Out-gated |
| `lod-object-*` | Conditional distant sprite/object instances | Out-gated |
| `rope-*` | Conditional entity tether geometry | Out-gated |
| `particle-*` | No instances with particles disabled | Out-gated |
| `fluid-*` | Conditional transparent terrain mesh | Out-gated |
| `debug-*` | Conditional debug geometry | Out-gated |
| `clouds-*` volumetric composition | Fullscreen source-color copy remains under BareMinimum | Out-gated |
| `trail-*` | No instances with weapon trails disabled | Out-gated |
| `dual-downsample-filtered`, `dual-downsample`, `dual-upsample` | Bloom passes absent with bloom Off | Out-gated |
| `postprocess-*` | Still submitted; internal scale 0.1 is upscaled | Out-gated; lines target full UI4 extent directly |
| `ui-*` Conrod HUD/ESC | TRUEOS drawing feature disabled | Out-gated |
| `premultiply-alpha-*` | UI resource maintenance may still submit it | Out-gated |
| `blit-*` | Screenshot route only | Out-gated; screenshots unsupported in this stage |
| Native Iced menus | Separate UI4 producers | Kept, including rules and existing-character selection |

Primary source points: `src/scene/mod.rs::Scene::render`,
`src/render/renderer/drawer.rs`, `src/render/renderer/pipeline_creation.rs`,
`src/scene/lod.rs`, and `trueos-bringup-profile.json`.

## Executing line route

1. Real client tick and terrain workers produce the original opaque meshes.
2. The existing terrain drawer supplies its culling-selected vertex slice,
   shared quad indices, global camera buffer, and chunk-local model matrix.
3. `trueos_host.rs` validates ranges and the shared index pattern. It decodes
   the 8-byte packed terrain vertex, including the signed Z bias of 32768.
4. Model, focus-origin subtraction, and camera projection match terrain shader
   position math. Only quads touching the camera's 64-metre neighborhood are
   considered. Extraction inspects at most 65,536 quads per draw and emits at
   most 65,536 vertices per frame. These are greedy-mesh quad perimeters, so
   merged block faces need not show every individual voxel edge.
5. Each perimeter segment is clipped against all six homogeneous frustum
   planes before perspective division. The native package receives finite NDC
   XYZ coordinates with depth in 0..1. Triangle diagonals are omitted.
6. At the normal drawer's final queue present, `trueos_lines.rs` uploads into
   retained vGPU buffers and submits the admitted
   `CLIP_POSITION3_IMMEDIATE_RGBA` package as `LINE_LIST`.
7. Exact queue retirement precedes UI4 background publication. Busy publication
   is retried before a new write lease; resize is deferred across pending
   publications. Empty geometry clears old outlines. The foreground producer
   is retired before gameplay scene presentation.

This produces a bounded wireframe view of real streamed terrain. It has no
sky, clouds, distant ground, sampled textures, filled terrain faces, or HUD.
The admitted direct line path has no depth allocation or depth test. Outlines
can show through faces: this is a wireframe diagnostic rather than a fully
occluded solid-world renderer. Native viewport orientation already matches
the camera; reversed depth stays in 0..1 without flipping Y or Z.

## Remaining gates and costs

- Original shader binaries, material/atlas sampling, multi-target/depth texture
  rendering, figure/sprite instancing, lighting and postprocessing still need
  explicit native admission and layouts. No arbitrary SPIR-V forwarding exists.
- Source CPU buffers have a shared 256 MiB budget with last-reference release.
  Texture uploads are excluded; they do not allocate native texture storage.
- CPU line vectors and GPU vertex/index buffers are retained. The existing kernel V2 path still
  copies ranges and creates a resident indexed mesh per submission. Zero-copy
  vVideoMem aliasing and asynchronous reclamation are deliberately deferred.
- CPU projection runs each scene frame. Meshes are retained in the existing
  resource flow, but the native GPU does not yet perform the camera transform.
- The existing-character selector has Play, Spectate, and Back controls.
  Character creation/editing/deletion remain out-gated in this selector.
- Queue retirement plus publication is logged for the first nonempty terrain
  frame. Background publication has no physical scanout receipt; a successful
  build or host test alone does not prove pixels reached the display.

Validation: isolated host tests exercise packed decoding, clipping, real draw
slice/index/base-vertex/binding offsets, quad perimeter order, mapped uploads,
buffer-budget release, restricted adapter negotiation, and surface ownership.
The direct line executor also compiles against the TRUEOS vAPI.
The final deployed artifact is built with `cargo bp voxy`.
