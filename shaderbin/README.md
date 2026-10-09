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


The native cloud renderer carries the last visible camera across selection/world
handoffs. It blends the eye, orientation and focal length in four 100 ms steps,
then follows the live camera directly. Orientation uses quaternion interpolation
so opposite viewing directions keep a valid basis. This affects only the cloud
view: terrain controls and the display background color retain their live paths.

Native terrain faces use 3x3 atlas tiles for the small color variations from
Voxygen's `terrain-frag.glsl` / `include/random.glsl`. The worker evaluates the
original chunk-local, normal-offset hash at the nine cell centers. Its sqrt-space
noise strength is increased by 10% (0.015 to 0.0165), normalized around our
existing axis-shaded base RGB
rather than importing the full renderer's lighting brightness. UVs interpolate
across each tile with nearest sampling and a tiny edge inset. The admitted
textured shader and six vertices per face stay unchanged. The atlas is now
1024x1024 (4 MiB), sufficient for all 100,000 budgeted faces plus the proxy tile;
identical nine-color tiles share storage. Camera movement does not reseed noise.
`tests/terrain-mesh/run.py` checks cell mapping, stable placement, negative/chunk
boundaries, deduplication, full-budget capacity and existing occlusion guards.
The host GPU proof in `tests/flat-clouds` also checks all nine sampled cells in a
full-sized atlas and cloud composition without neighboring tile bleed.

World wheel zoom now follows one continuous distance curve from first person
through third person to a full nine-chunk overview. Offset exponential wheel
math provides fine near-player control and faster travel farther out; easing
uses real elapsed time and cannot overshoot on a slow frame. Mode changes do
not reset distance to 2.35 blocks. Coalesced input is limited to one notch and
invalid numeric input is ignored. The overview limit encloses the 96x96
footprint about any player position in its center chunk, includes the full mesh
height span plus head/jump clearance, and accounts for FoV/aspect with 15%
margin. The limit does not depend on live player height: jumping or stepping
moves the camera focus without retargeting overview zoom. The terrain far plane grows to
cover that overview. The camera-mode key uses the same eased distance flow.

The integrated terrain mesh can cover the graphics minimum's complete 3x3
chunk area (96x96 blocks), aligned to the player's current chunk, once a
contiguous rectangle of real chunks has arrived. Vertical
bounds come from the received chunks' stored height extents and face-neighbor
transitions, rather than a 64-block crop around the player. Unknown chunks
still suppress boundary faces. Moving inside a chunk no longer remeshes the
terrain; chunk crossings, arrivals, removals and copy-on-write edits do. The
existing 600,000-vertex GPU budget remains enforced and logged on truncation.

## Retained near + whole-map terrain experiment

The full-client bring-up defaults to **Both**. Global F1 cycles
**Both → Far → Near → Both**, before menu/game bindings; held-key repeats do
not cycle. Clouds keep the existing flat pass. F1 changes the rendered terrain
layers; CPU snapshots remain warm for returning to the other mode.

| Layer | Source / technique | Initial budget |
| --- | --- | --- |
| Near | Real received voxel chunks, exposed block faces, existing 3×3 noise tiles | VD=1: at most 9 chunks / 96×96 blocks; 600,000 total terrain vertices |
| Far | Connection-time `WorldData.lod_alt` via `alt_at()` and `lod_base`; one uniform whole-map triangle lattice | Guess 32,768 cells, halve to **16,384**; at most 98,304 uncut vertices, 120,000 including cut triangulation |
| Join | Exact rectangle subtraction on each far triangle; boundary skirts follow the same triangle planes | Combined geometry stays within 600,000 vertices |

Only a hole-free rectangle of real chunks can become near coverage. Initially
this can be the central 32×32 chunk; arrival of neighboring strips expands it.
Coverage and geometry commit from the same worker snapshot. Truncated meshes
do not punch holes in Far. At the combined vertex limit, Both falls back to the
complete far map and reports `budget_fallback=true`. This fallback is a failed
performance acceptance result, not a successful close terrain demonstration.
The cut follows completed coverage, not the requested VD square. It therefore
also handles rectangular intermediate coverage without drawing the ground twice.
VD remains 1 for this experiment; enlarging the network request does not yet
enlarge the rich mesh radius.

| Function / boundary | Inputs and transform | Execution |
| --- | --- | --- |
| `Client::handle_server_terrain_msg` / `poll_terrain_decode` | Stamp application receipt before ordered decoding; install chunk plus its receipt atomically on successful decode | CPU; one decode in flight, bounded existing message queue |
| `Scene::prepare_terrain` / `terrain_mesh` | Chunk snapshot, player chunk, actual stored height bounds → exposed faces + palette; at most one worker, minimum 500 ms between starts | Background CPU |
| `Composition::prepare` / `snapshot` | At most 16,384 height/color samples from the already available full map; no extra voxel requests | Bounded CPU snapshot; map validation and composition workers |
| `terrain_layers::compose(map, near, coverage, mode)` | Subtract four rectangle half-planes with interpolated Z, triangulate outside polygons, join boundary height planes, reserve atlas rows 896+ for far colors | Background CPU; maximum one composition worker; minimum 100 ms between starts |
| `NativeTerrain::draw(target, frame, load_color)` | Retained 32-byte vertices + u32 indices + RGBA atlas; update the 80-byte eye/basis/FoV/aspect/far camera block each frame | App vGPU submission, one combined terrain draw after clouds |
| Kernel `prepare_voxy_stream_mesh_raw` | Dirty vertex/index ranges and camera → existing resident Intel streaming mesh; atlas retained separately | Driver; unchanged Voxy admission and shader package |
| `VoxyHeadlessTexture` contract | World coordinates → camera-relative projection; interpolate palette UV → sampled RGBA; drawable depth test/write | Intel GPU graphics shaders; GuC schedules work, it does not decode terrain |
| `Device::wait` → scene `publish` | Retired render timeline → paired UI4 background publication | GPU completion and compositor handoff; no physical scanout receipt |

Near meshing and far clipping are CPU algorithms in this first integration.
Rendering and retained geometry/texture reuse are GPU work. This is not yet
GPU voxel compaction or compute-generated far geometry; introducing that needs
an admitted compute/shader contract and a measured improvement over this baseline.
The full native session uses vGPU directly, rather than passing these draws
through the general wgpu API.

`terrain-heartbeat` emits at IMPORTANT once per two-second wall-clock window,
even during presentation retries. It carries a run ID and sequence number,
actual mode/revision, extent, loaded vs proven near chunks, vertex counts,
application buffer-write bytes, warm/compose time, preparation and
submission+wait p95, frame-start-to-UI-publication p50/p95/max, busy retries and
worker failures. A new revision's receipt-to-first-publication latency is sampled
once. `ready_chunk_frames` measures retained ready coverage per published frame;
it is explicitly not a count of newly decoded chunks. No frames means zero
throughput. F1 starts a new window; recovery does not erase cumulative failures.

The two-second `throughput_ok` indicator is provisional (at least 27 FPS,
p95 ≤33,333 µs, available layers, no fallback/new worker failure). The stricter
capture checker requires **30 consecutive seconds per mode at ≥29 FPS**, a
common resolution, all nine near chunks where applicable, p95 ≤33,333 µs and
camera-only app writes in steady windows. Missing sequence numbers, stalls,
partial coverage and fallbacks break the consecutive proof. By default it also
requires actual receipt samples in Near and Both, provisionally ≤2 seconds;
the warm-crossing option below supplies readiness evidence for prefetched data.
These are initial experiment gates, not established Intel throughput numbers.

For a fresh hardware run: connect in Both, warm the resident buffer, then
walk through at least five ordinary chunk crossings (including diagonal movement
and direction changes). Stop and capture at least 32 steady seconds in each of
Both, Near and Far, at the same resolution, in one uninterrupted capture. Use
`python3 tools/terrain_metrics.py <capture.log> --require-warm-crossings 5`.
This gate checks resident data at crossings plus steady rendering in each mode.
It joins evidence by terrain-stream ID, requires zero cold ordinary crossings
and a full final warm ring, and does not demand new network arrivals for terrain
that was deliberately prefetched. The original receipt-based gate remains
available without the flag for experiments that consume arriving chunks directly.
The checker outputs JSON and exits nonzero if evidence is absent or fails;
physical display proof remains unavailable. Test resize, disconnect, partial
arrivals and rapid F1 separately. Local host tests execute no Intel workload.

## Ahead-of-use decoded terrain

The VD=1 full client now warms a **5×5 resident area** around the same **3×3
rendered area**: up to 25 decoded chunks, including 16 speculative neighbors.
`TerrainChunkRequest { key }` already accepts coordinates in a larger radial
allowance than the current request loop. The warm ring uses that allowance with
a one-block position margin. Tests compile the actual server admission expression
and check requests at chunk edges and with small server-position lag. No protocol,
server code, VD negotiation, or entity distance changes are needed.

Current near chunks get priority. After all nine are resident, the warm scheduler
requests at most one additional chunk per 100 ms, with at most six speculative
requests pending inside the existing twelve-request total. It pauses when near
coverage is incomplete or the decode queue is backed up. Already queued/in-flight
chunk decodes suppress duplicate requests, even after a request timeout. Failed
speculative replies back off three seconds. Long frames cannot create a catch-up
burst. Outside-map default data is promoted locally without a network request.

The scheduler estimates when each candidate enters the near rectangle using
position within the current chunk and planar velocity. At a diagonal crossing
it prioritizes the required corner over strips the player is leaving. Other
directions fill too, so turning does not require inventing a new request path.
The existing unload hysteresis retains the entire ±2 warm ring plus a trailing
margin, with at most 45 resident keys after pruning. The warm ring covers the
next rendered 3×3 after a single neighboring chunk move when those targets are
resident. Render meshes still follow the existing preparation/upload budget.

The two-second `terrain-prefetch` heartbeat reports decoded near/warm coverage,
pending requests and decode backlog, warm/cold ordinary crossings, separately
counted startup/teleport behavior, and request-to-decoded p95 over up to 32 actual
completions. `next_missing_eta_s` exposes the predicted urgency of remaining
candidates. Startup is not counted as a movement miss; multi-chunk jumps are
reported as teleports. A normal crossing with any missing near data increments
`cold_crossings` permanently for that stream, so later recovery cannot hide it.
Host models cover straight and diagonal movement at 8 blocks/second with an
assumed 1.5-second request-to-decoded delay. This is scheduler evidence, not a
measured latency or a guarantee under arbitrary movement/network conditions.
