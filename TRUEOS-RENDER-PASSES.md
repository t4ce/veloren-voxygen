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
- Queue retirement and publication are logged separately from exact background
  SURFLIVE proof. That proof requires the updated kernel receipt implementation;
  a successful build or host test alone does not prove pixels reached the display.

Validation: isolated host tests exercise packed decoding, clipping, real draw
slice/index/base-vertex/binding offsets, quad perimeter order, mapped uploads,
buffer-budget release, restricted adapter negotiation, and surface ownership.
The direct line executor also compiles against the TRUEOS vAPI.
The final deployed artifact is built with `cargo bp voxy`.

## Scene startup failure reporting

A `Creating render pipeline 0/0` display previously meant the entire scene-init
worker was pending, not that shader compilation had started. The first rig
run failed its intermediate-format descriptor check before pipeline creation.
The host graph now admits RGBA8 intermediate descriptor copy/filter usages;
this remains metadata support, not native sampled rendering. Unsupported
intermediate formats and native line admission return startup errors to the
menu instead of panicking in the worker.

Scene-init completion uses a bounded result mailbox rather than joining or
polling thread teardown. Named startup stages are shown and logged: bridge
negotiation, format checks, line GPU admission, scene descriptors, pipeline
handles, and remaining scene resources. Initial TRUEOS pipeline handles are
created sequentially on the existing scene-init worker; no extra compilation
pool is needed for excluded shaders.


## Authentication before graphics

A login click starts the existing ClientInit directly. Server discovery,
handshake, authentication, and registration acceptance happen before scene
startup. Authentication failures return through the normal login error UI
without allocating the scene GPU device or preparing pipeline handles.
ClientInit itself rejects authentication before loading GameSync initial data.
Only a successfully accepted client starts the scene-init worker; its network
tick continues while that worker prepares graphics. The client and scene
transition are dropped if the user cancels or graphics returns an error.

## Client first-tick bringup

`Prepare Client` can remain the last painted message after ClientInit completes:
the main menu ticks the accepted client synchronously while graphics initializes.
`ecs-dispatch-enter` from that client therefore identifies simulation dispatch,
not map generation or shader compilation.

TRUEOS client ECS and unscoped parallel helpers share the existing client runtime
through `State::pools_on`, avoiding separate ECS and default iterator runtimes.
Client dispatch uses the dispatcher's sequential dependency stages inside that
pool's install scope. Every system still runs; internal parallel operations are
retained. Server dispatch and desktop clients retain their previous scheduling.
This is a bringup scheduling workaround, not a proven diagnosis of the rig hang.
Each system logs its first entry and completion, including its origin, so the
next run can attribute a remaining stall. Restore concurrent client system groups
after the TRUEOS carrier/scheduler behavior is established.

## Paired handoff and physical scene receipt

The menu's background worker and the terrain producer share the same scene
capability. Before terrain draws begin, `Window::prepare_scene_display` drains
the last submitted **background** menu revision as well as the foreground
clear revision. No new menu work is admitted during the handoff. A window
resize submits a fresh transparent foreground publication without restarting
the menu background producer; terrain supplies the resized scene publication.
Producer failures return an error instead of leaving this gate silently waiting.
The drain starts when the real session records its first scene frame. Native
character selection continues to own the menu layers and does not drain them.

`LinePresenter` retains the identity of its completed frame (sequence, vertex
count, and extent) through publication retries. Background tracked publication
and `was_presented` now use the existing CABI signatures, with a separate
background serial history in the broker. The compositor records that exact
serial after the same SURFLIVE commit that retires its display lease. Foreground
history and paired resize staging are unchanged.

On a kernel with the new receipt implementation, a tracked scene frame is held
until its receipt is observed. An unobserved receipt times out after two seconds
without claiming success and allows a newer frame. A paired resize uses the
ordinary atomic publication path, then resumes tracked frames. An older kernel
reports that background receipts are unavailable and keeps ordinary rendering
and publication enabled. **Boot the rebuilt kernel to use exact background
SURFLIVE proof; rebuilding only the Blueprint cannot supply that kernel code.**

Runtime evidence is separated into `Voxygen terrain source` (draws, candidates,
near quads, emitted segments and truncation), `first native frame retired and
published`, and `Voxygen terrain SURFLIVE proven`. Only the last message records
an actual receipt for nonempty terrain geometry. None of the host tests or pack
commands produces that claim.

## Loading-image frontier after the first ECS tick

The fresh rig runs of `a8160f...` and `7d5d89...` reached native line admission,
host pipeline handles, and `Finishing scene resources`. All listed first-run
ECS systems returned. These are independent worker and main-thread boundaries:
the main menu ticks the authenticated client before polling the scene worker.
Neither record establishes that a world frame has reached the line executor.

TRUEOS character selection now constructs only its native selector and client
state. It no longer constructs the original simple preview scene, joins sprite
preparation, or constructs/maintains the legacy Conrod character UI. Desktop
character selection retains its original behavior. The real session still
creates the regular scene, terrain meshes and client tick loop.

The original noise texture is a 1x1 descriptor placeholder during the native
line stage; every shader pass that samples it is gated. Sprite preparation still
produces the same meshes and atlas, but TRUEOS consumes its bounded result
mailbox rather than waiting for thread teardown. These changes preserve the
terrain and native XYZ/12-byte/LINE_LIST contracts.

New first-tick records cover ECS maintenance, terrain application, local events,
client terrain requests, and client return. Startup records distinguish graphics
state completion, renderer delivery/acceptance, native selector construction,
session scene resources, sprite mesh arrival, and menu revision drain. Handoff
waits report the required and published revisions at most every two seconds.
The exact cause of the recorded boot's stall remains unproven until these
boundaries are observed on the rig; the selector prerequisites were unnecessary
regardless of which boundary that run stopped at.

## Default pack: opaque black scene

Native geometry is now excluded by the opt-in `trueos-native-lines` feature.
The default pack does not construct the line presenter, open its vGPU device,
allocate vertex/index buffers, create a line shader/pipeline, extract/project
terrain edges or submit indexed geometry. It retains the ordinary client and
state loop. The existing UI4 scene producer clears opaque black (`0xff000000`)
with an empty sprite batch and publishes through its existing Busy/retry loop.
The capped world render loop queues a fresh black frame every tick. Only these
explicit black-clear jobs bypass the unchanged-plan shortcut. Main-menu and
loader background images pass through unchanged, and menu jobs retain their
normal cached-publication behavior. No original 3D frame is recorded in this
mode. Entering a world clears the menu foreground.
Both scene and foreground still publish when paired resize needs new buffers.

This is a black-frame bring-up, not proof of terrain pixels. The dormant line
executor also has an unresolved error-contract bug: a rejected native render
submission can return Busy safely, while its caller promotes the error to a
panic. Keep the feature off until that contract is fixed.

The native character selector is a temporary replacement for the excluded
character-preview/Conrod screen, not scene-rendered text. It now uses the
existing menu background and button skins in a centered panel. Internal
bring-up labels have been removed. Character creation/editing/deletion remain
excluded; existing characters and Spectate still use the real client requests.

Back previously popped the state and synchronously ran `Client::drop`, which
called `Runtime::block_on(Participant::disconnect())` on the input/Hull thread.
The network's graceful flush can wait up to 120 seconds. TRUEOS now transfers
the participant and network into an asynchronous runtime task: input returns
immediately, ownership is retained while disconnect completes, and network
destruction runs in an async context where scheduler acknowledgement is
deferred. Desktop teardown remains unchanged. The recorded run reaches native
selection but contains no kernel panic record; it does not establish the exact
kernel failure behind the reported freeze.

## Selector transition and cooperative stop

The `a227e832...` capture advances through client first-tick completion, scene
renderer readiness, main-loop acceptance and native selector construction. It
does not establish a terrain draw or a native selector publication. New kernel
records distinguish server-rule construction/skipping, native screen entry,
the first queued UI plan and the first layout/presentation error. The TRUEOS
rules screen also omits the unused original preview scene and raw-font preload;
the native rules UI and desktop path remain intact.

That run ends at `stop requested cooperative=0 native_jobs=8`, followed by
`draining vm=0 jobs=8 resources=retained`. Voxy had not registered cooperative
cleanup: forced stop ended Hull execution before its Rust destructors could
stop the UI producers, runtime and process workers. The pending native jobs
therefore prevented guest resource reclamation. This is evidence of blocked
teardown, not proof of a BSP spinlock or of the preceding visual failure.

The existing shutdown guard is now registered before process workers and
dropped last. Safe event-loop callbacks observe Stop and exit; play states drop
before their window/runtime, both UI producers receive Stop before either
joins, and retained scene/sprite preparation workers join while their runtime
is alive. The shared runtime is installed before early sprite work can create
a persistent fallback pool. No timeout frees memory belonging to live workers.

On the next rig run, look for `Voxygen cooperative shutdown: registered`,
`stop requested cooperative=1`, `Voxygen cooperative stop`, producer join
records and the normal completed native-job drain. This pack cannot retrofit
cooperative cleanup into the already stopped guest. Host regression coverage
checks the actual paired-producer destructor ordering; a successful pack does
not establish either a physical restart or terrain SURFLIVE.
