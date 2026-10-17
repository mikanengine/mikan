# Mikan status

- **Date:** 2026-10-17
- **Revision:** `main` @ `99fcbb6` ("verification: force the X11 backend in the
  input verify scripts"), the last pushed commit, plus local, unpushed commits:
  `cb27f88` (the Phase 8 CI record), `a3f77e8` (the Phase 9 audio milestone —
  the audio-sys and audio crates, the WAV decoder, the demo2d `--audio` lane,
  the bench and verify scripts, ADR 0014, `docs/audio.md`, and the refreshed
  README, ARCHITECTURE, DEVELOPMENT, CHANGELOG, FFI, and STATUS text), and this
  commit (which records them here). The earlier history is described in the
  previous revision of this header: the Phase 3 change set as eight commits
  (`991c17e`..`5c95578`), Phase 4 (ECS-lite) as five
  (`813cbc2`..`0f74e34`), Phase 5 (collision + Luau scripting) as `5f63579`,
  the Phase 7 (scene and entity system) change set plus the crate-directory
  renames (`crates/mikan-*` → `crates/*`) as `860d698` and its follow-up
  kernel-driver fix as `07c6c33`, and the Phase 8 input work as `773589e` (the
  bulk) and `23be31e` (the C++-runtime release fix, the build-resource tuning,
  and the documentation). **Pushes and CI runs still require explicit
  authorization**, so the "Verification: CI" section below records the last
  pushed state, and every claim in this document comes from local runs on
  this machine.
- **Purpose:** state precisely what is implemented, how each claim was verified,
  and what is known to be missing. Verification levels follow ADR 0005: nothing
  is called *verified* unless it was run.

## Milestone: first playable 2D vertical slice — complete

A window opens, presents rendered frames at the requested presentation mode,
reads the keyboard, runs a fixed-timestep simulation with interpolated
rendering, survives resize and occlusion, and exits cleanly. Applications are
driven by the engine's own runtime: timing and configuration from `mikan-core`,
window lifecycle and input from `mikan-platform`, device/surface/present and
drawing from `mikan-render`.

## Milestone: textured sprites and a minimal 2D camera — complete

`Quad2d` was replaced by `Sprite`: a rectangle in world space that is either
flat-colored or samples a texture through an atlas-ready UV rectangle. Textures
are uploaded from linear RGBA8 data after validation, drawn with one instanced
draw call per consecutive run of sprites sharing a texture (painter order equals
submission order, sprites are never reordered), and rendered through an optional
`Camera2d` (position and zoom, defined in `mikan-core`, applied in
`mikan-render` as a view/projection matrix). A frame without a camera renders in
pixel space through `Camera2d::pixel_space`, reproducing the previous milestone
exactly. The milestone contains **no C or C++** — ADR 0006 records why the
native-module criteria (ADR 0004, `docs/ffi.md`) are unmet.

## Milestone: the engine facade crate `mikan` — complete

Applications no longer wire the platform and render layers together by hand.
`crates/mikan` provides the `Game` trait, an `Engine` handle carrying the
platform context plus the renderer, and `mikan::run`. Engine and application
failures share one channel; `Engine` hands out `&Renderer` for queries and
texture creation and a `render_frame` call for submission. `demo2d` is a client
that depends on the single `mikan` crate, and the milestone contains **no C or
C++** — ADR 0007 records why the ADR 0004 criteria remain unmet.

## Milestone: 2D sprite rendering system (assets, samplers, pixel-perfect) — complete

Real assets replace code-generated tiles, and the sprite pipeline gains the
controls a 2D renderer needs:

- **`mikan-assets`**, a new CPU-only crate (`mikan-core` + `png` + `serde` +
  `serde_json`, no graphics dependency): `Image` decodes every PNG color type to
  RGBA8 with per-side/allocation limits, and `SpriteSheet`/`SheetFrame` read
  named frames from a documented Aseprite JSON subset with load-time validation
  (duplicate names, zero sizes, out-of-bounds rectangles, non-finite or
  out-of-range pivots, and explicit rejection of rotated/trimmed frames).
  `SheetFrame::uv` insets half a texel per side so linear sampling cannot read a
  neighbouring frame — the atlas-bleeding fix — and a one-texel frame maps to
  its texel center. The format is documented in `docs/sprite-sheets.md`.
- **Textures declare their encoding** (`TextureEncoding::{Linear, Srgb}`):
  procedural data stays exactly as before, and PNG bytes are uploaded as sRGB,
  which the GPU decodes on sample, so the colors a PNG author saw are the colors
  that render.
- **Rotation, tint, and alpha are per-sprite data**: `Sprite::with_rotation`
  bakes `(cos, sin)` into a 56-byte instance; the shader rotates the quad and
  multiplies the sampled texel by the linear tint with straight-alpha blending.
- **Per-sprite sampling** (`SamplerMode::{Linear, Nearest}`) is part of the
  batch key; every texture carries one bind group per mode, so crisp-vs-smooth
  never costs a pipeline switch, and the two modes are visibly different in a
  capture.
- **Draw statistics are observable**: `Renderer::last_frame_stats()` returns
  `FrameStats { sprites, batches, draw_calls }`, which is what the verification
  scripts grep instead of trusting prose.
- **Pixel-perfect mode**: `Camera2d::pixel_perfect` derives the largest integer
  zoom at which a virtual screen fits the window and snaps the camera to the
  physical pixel grid; the contract and its limits (≤1 px centering, no
  letterbox, sprites must be aligned by the game) are recorded in ADR 0008.
- **The demo loads real files**: `games/demo2d/assets/{atlas.png, atlas.json,
  star.png}` are committed, produced deterministically by
  `scripts/generate_assets.py` (stdlib only, `--check` for CI), and loaded
  through `mikan-assets` at startup — a missing, corrupt, or incomplete set
  stops the demo with a readable error before the first frame. Three scenes
  (`classic`, `showcase`, `stress`) and new options (`--scene`, `--sampler`,
  `--pixel-perfect`, `--assets`, `--size`, `--no-vsync`, `--sprites`) exercise
  them.
- **The 10,000-sprite benchmark is a script, not a slogan**:
  `scripts/bench_sprites.sh` measures with vsync off, a fixed frame budget, and
  two window sizes, and the numbers (including the software-rasterizer caveat)
  are recorded below. 10,000 sprites in one batch and one draw call is verified;
  60 FPS is a target, not a claim, and it is not reached on this machine's
  software rasterizer.
- The milestone contains **no C or C++** — ADR 0008 records why the ADR 0004
  criteria remain unmet; its benchmark points at rasterization throughput, not
  at a boundary Rust expresses badly.

## Milestone: ECS-lite (entities, components, resources, scheduled systems) — complete

Gameplay state moves into a small, dependency-free ECS:

- **`mikan-ecs`**, a new CPU-only crate (`thiserror` only; no workspace
  dependency): `Entity` generational handles (`Display` as
  `{index}v{generation}`, LIFO slot reuse, stale handles never resolve),
  `World` with one insertion-ordered sparse-set storage per component type
  keyed by `TypeId`, typed resources (`insert_resource` returns the previous
  value), and `iter`/`iter_mut`/`iter2`/`iter2_mut` queries. Removal preserves
  insertion order — an O(n) trade, stated in `docs/ecs.md` — and `iter2_mut`
  proves its two storages disjoint with `HashMap::get_disjoint_mut`, so the
  crate needs no `unsafe` and `iter2_mut::<A, A>` panics loudly instead of
  aliasing. `DeadEntity` is the one error.
- **`Schedule`**: named systems run once each, in registration order, with the
  `dt` the caller passes — no dependency graph, no parallelism, no timer of its
  own. The platform's `FixedTimestep` remains the only thing that decides when
  and how often the schedule runs.
- **Determinism is tested, not promised**: `tests/determinism.rs` reproduces
  positions and interpolation state bit for bit for a given binary; the
  cross-target floating-point caveat is documented on `Schedule::run` and in
  `docs/ecs.md`.
- **The demo migrated**: the classic scene's player is an entity with
  `Position`, `PreviousPosition`, `Motion`, and `Player` components, resources
  `MoveDirection` and `WorldBounds`, and systems `snapshot, input, movement,
  clamp` running once per fixed step in exactly that order. The per-step
  arithmetic is unchanged bit for bit; `render_pose` is pure (rendering never
  mutates the simulation); `games/demo2d/src/square.rs` is replaced by
  `games/demo2d/src/player.rs`.
- **End-to-end verification**: `scripts/verify_ecs.sh` requires the startup
  log's entity/component/system line, requires the shutdown moved-step counter
  and the final position to agree exactly (3.5 px per moved step, until the
  wall clamps the player), and requires a no-input run to keep the counter at
  zero. Wired into `scripts/verify.sh` and the CI smoke job.
- The milestone contains **no C or C++** — ADR 0009 records why the ADR 0004
  criteria remain unmet: the subsystem's value is compile-time-enforced handle
  validity and non-aliasing over a wide, allocation-heavy data structure, which
  an FFI boundary would weaken, and there is no native implementation to
  measure against.

## Milestone: 2D collision detection and resolution — complete

The roadmap's Phase 5 milestone, with the narrow-phase kernel in C:

- **`mikan-collision-sys`**, one of the two crates allowed to use `unsafe`: a C99
  narrow-phase kernel (`src/mikan_contacts.h`, `src/contacts.c`) compiled by a
  `build.rs` with warnings-as-errors, `#[repr(C)]` mirror types, and the raw
  `extern "C"` declaration (public with `# Safety` docs so tests can exercise
  the C side's defensive behavior). One batch function, `mikan_contacts`,
  writes a `Contact` per pair: no allocation, no retained pointers, no state,
  no error channel. The lint override and its reason are recorded in the crate
  root.
- **`mikan-collision`**, the safe face (no `unsafe`): `Aabb`/`Pair`/`Contact`
  with documented semantics — overlap means `depth > 0`, touching boxes are
  separated, the normal points `first`→`second`, and non-finite coordinates
  yield separated results order-independently; a uniform-grid `Broadphase`
  whose output is sorted and deduplicated (deterministic regardless of hash
  order, cells reused across steps); the safe batch `contacts` call; and
  `reference`, a Rust implementation of the same math kept as the test oracle
  and benchmark baseline.
- **Cross-checks are tests, not prose**: the C kernel and the Rust reference
  are compared bit for bit on fixed and randomized cases, including a 256-box
  scene stepped 8 × 20,000 times.
- **The demo integrates it**: three static blocks (one unit test guards that
  none touches the y = 300 corridor the input and camera checks walk), the
  player is a collider too, and a stateful `collide` system rebuilds the broad
  phase and resolves contacts once per fixed step — movable bodies (those with
  `Motion`) are pushed out, half the depth each when both are movable, and
  anything without `Motion` stays put. The schedule gained one system:
  `snapshot, input, movement, collide, clamp`.
- **The measurement is honest**: `scripts/bench_collision.sh` runs 250,000
  candidate pairs from 512 boxes through both implementations in release; the
  recorded result is **parity within run-to-run noise** (one run favors C, the
  next favors Rust), and no speedup is claimed. A Valgrind run of the
  benchmark binary reports **0 errors and 0 bytes lost**.
- **The decision is explicit**: the kernel exists because the low-level numeric
  layer is C's assigned home in Mikan's architecture, not because of a profiled
  bottleneck; ADR 0010 records that, reports the parity result, and supersedes
  ADR 0004 *for this module only*.
- **End-to-end verification**: `scripts/verify_collision.sh` requires the
  startup facts, then holds down+right and requires the run to stop against the
  first pillar's left face (x = 582) and slide down it to the floor clamp
  (y = 552), and finally requires the y = 300 corridor to stay unobstructed.
  Wired into `scripts/verify.sh` and the CI smoke job.

## Milestone: Luau gameplay scripting — complete (owner-directed roadmap amendment)

The roadmap's Phase 5 is collision; during the phase the project owner directed
that Mikan's stated architecture — Rust + C++ forming the engine core, C as the
low-level foundation — is a requirement, and this is where it starts being
real. Luau gameplay scripting is that second half, and ADR 0011 records it as an
amendment to the roadmap, not a silent substitution:

- **`mikan-script`** (safe Rust; `mlua` + vendored Luau): `ScriptEngine` loads a
  script from a file or source and calls its global
  `mikan_step(dt, left, right, up, down, pos_x, pos_y)` once per fixed step,
  returning a direction and a speed. The engine validates every return: finite
  in `f32`, non-negative speed, direction longer than unit length normalized
  (shorter ones are left alone for deliberate slow walks). A `used_memory()`
  accessor exposes the 64 MiB limit's headroom.
- **The sandbox is explicit**: coroutine, table, string, math, utf8, bit32, and
  Luau's side-effect-free `os` only — no I/O, `debug`, package loading, or FFI
  — plus `lua.sandbox(true)` and a 64 MiB memory limit. mlua is pinned at
  0.11.6 because 0.12 requires a newer Rust than the workspace MSRV (1.87).
- **The C++ subsystem is the vendored Luau VM**: compiled by `mlua-sys` from
  pinned upstream sources, so a C++ compiler is now a build prerequisite.
  Mikan writes no C++ glue.
- **Failure semantics are split and tested**: unreadable file, compile error,
  and missing `mikan_step` are load-time failures (fatal; the demo exits 1 with
  the script name and line), while an error raised inside `mikan_step`, a wrong
  return count or type, a non-finite value, or a negative speed fail that one
  step (`Runtime`/`InvalidReturn`); the demo logs the first one and idles for
  the step while the game keeps running.
- **The demo's movement is the script**: `--script PATH` (default
  `games/demo2d/scripts/player.luau`) is loaded at startup; `update` reads the
  held keys, the position, and the fixed step, calls `call_step`, and writes
  the result into `Controls` before the (unchanged) schedule runs. The default
  script reproduces the classic player exactly: 420 px/s, unaccelerated
  diagonals.
- **End-to-end verification**: `scripts/verify_script.sh` — the default script
  moves exactly 3.5 px per moved step; a script returning `(3, 4, 60)` for
  exactly 60 steps ends at `[498, 324]` (the normalized `(0.6, 0.8)` path;
  unnormalized would be `[570, 420]` and a broken counter would clamp at
  `[912, 552]`); three load-time failures exit 1 with file:line; a runtime error
  and a non-finite return are each logged exactly once, move nothing, and leave
  the game running. Wired into `scripts/verify.sh` and the CI smoke job.

## Milestone: scene and entity transform hierarchy — complete

The layer above the ECS: where each entity is, what its parent is, what a scene
is, and how deferred despawn works. It is the first module with hand-written C++
of Mikan's own, beside a C numeric kernel, with the ECS, scene policy, and
manager kept in Rust (ADR 0012):

- **`mikan-scene-sys`**, one of the three crates allowed to use `unsafe`: a C compose
  kernel (`src/transform.c`, `mikan_transform2d_compose`, one batch-shaped
  function over a seven-`f32` `#[repr(C)]` struct) and a hand-written C++ scene
  graph (`src/scene_graph.cpp`, one opaque `mikan_scene_graph` handle with
  `extern "C"` entry points and status codes), compiled by a `build.rs` with
  warnings-as-errors and `-fno-exceptions -fno-rtti`. The graph stores topology
  and opaque `u64` identity only; `propagate` walks parents-first and delegates
  every arithmetic operation to the C kernel. No panics cross the boundary; the
  lint override and its reason are recorded in the crate root.
- **`mikan-scene`**, the safe face (no `unsafe`): `Transform2d` and
  `GlobalTransform2d` (rotation is the source of truth; the composed `(cos, sin)`
  pair is cached so the kernel never computes trigonometry), `Hierarchy` (a
  forest over entity handles: at most one parent, node `0` is the "no parent"
  sentinel, cycles are rejected without mutating the graph, removing a node
  re-roots its children, attachment and insertion order are stable), the
  `DespawnQueue`, `Scene` (one `World`, one `Schedule`, and the fixed update
  order: systems → `flush_despawns` → `propagate_transforms`), `SceneManager`
  (generational `SceneId`s, exactly one active scene per update), and
  `reference`, the independent Rust compose kept as the test oracle.
- **Cross-checks are tests, not prose**: the C kernel and the Rust reference are
  compared bit for bit (through `to_bits`, which distinguishes `-0.0`), including
  a large randomized hierarchy; `mikan-scene`'s propagation is checked against a
  reference fold as well.
- **The demo integrates it**: `--scene hierarchy` builds a sun (root), three
  planets (children of the sun), and a moon (child of the first planet) as
  entities with `Transform2d` components and the shared `SpriteRenderer`; `A`/`D`
  spin the sun so the planets orbit rigidly, and `F` despawns the moon through
  the deferred queue. The classic and hierarchy scenes draw through one
  `renderable::collect` walk over `(SpriteRenderer, GlobalTransform2d)`.
- **The measurement is honest**: `scripts/bench_scene.sh` runs the same
  hierarchy through the native graph and a Rust reference fold in release; the
  recorded result is that **the native graph is materially slower** (about
  fifteen times the per-node cost at 100,000 nodes) because it carries real graph
  bookkeeping, and no speedup is claimed. A Valgrind run of the benchmark binary
  reports **0 errors and 0 bytes lost**.
- **The decision is explicit**: the module exists because the low-level numeric
  kernel is C's assigned home and a dynamic graph is C++'s, not because of a
  profiled bottleneck; ADR 0012 records that, reports the slower-than-Rust
  result, and supersedes ADR 0004 *for this module only*.
- **End-to-end verification**: `scripts/verify_scene.sh` requires the startup
  layout (warm sun at `[480, 300]`, blue planet at `[640, 300]`, yellow star at
  `[720, 300]`), holds `D` and requires the moon's centroid to stay one orbit
  radius (240 px) from the sun while its angle changes by more than 5°, and
  presses `F` and requires the star pixels to disappear while the sun and
  planets remain. Wired into `scripts/verify.sh` and the CI smoke job.

## Milestone: the input system (action mapping over one event log) — complete

The layer that turns window events into gameplay answers: what an input *means*
(action mapping) instead of what a key did. It is the milestone where a native
split was the starting design, and the third boundary with both C and C++ of
Mikan's own (ADR 0013):

- **`mikan-input-sys`**, the third crate allowed to use `unsafe`: a C device
  backend (`src/input_backend.c` — the canonical event model over USB HID usage
  IDs, a bounded event buffer, and the raw device state: duplicate
  press/release suppression, OS-repeat normalization, move and wheel-delta
  coalescing, synthetic releases on focus loss) and a hand-written C++ action
  runtime (`src/input_runtime.cpp` — bindings grouped into prioritized
  contexts, per-event arbitration with later-created tie-breaks, and the
  per-action state machine: `held`, `just_pressed`, `just_released`,
  `repeating`, `value = held·1.0 + this step's wheel deltas`). The two native
  layers never call each other; Rust drains one and feeds the other in one
  batch per fixed step, and statuses (`OK/BAD_ARGUMENT/NOT_FOUND/CONFLICT/
  OVERFLOW/UNKNOWN_CONTEXT`) are the only language they share.
- **`mikan-input`**, the safe face (no `unsafe`): `InputSystem` owning both
  native halves, translating `DeviceEvent`s through the winit → HID map and
  mapping statuses to `InputError::{UnknownKey, DuplicateBinding,
  BindingNotFound, UnknownContext, ContextActive, Overflow, Native}`;
  `ActionId`/`ContextId` (ids start at 1, never reused), `Binding`
  `{Key, Button, Wheel(WheelAxis)}`, and `WheelAxis::{Horizontal, Vertical}`
  (the one device kind where code `0` is a real binding).
- **`mikan-platform` grew the front door**: the per-frame `DeviceEvent` log
  gained `take_events` (a true drain), wheel-tick normalization to pixels
  (`LineDelta` at 24 px), a bounded drop-and-count mode (`dropped_events()`),
  and `end_frame` clearing the log, wheel residual, and edges.
  `AppContext` now lends `&mut InputState` so one step can both read the raw
  lane and drain the log; the raw lane's behavior is unchanged.
- **Cross-checks are tests, not prose**: the C backend and the independent
  Rust `Normalizer` oracle are compared bit for bit on deterministic and
  randomized streams; the C++ runtime's arbitration, edge, wheel-value, and
  focus-cleanup semantics are asserted one behavior per test.
- **The demo integrates it**: `--actions` runs both lanes side by side — the
  raw lane (mandatory) and an `ActionMapping` (`global` context, priority 10:
  Escape → quit; `gameplay` context, priority 0: WASD/arrows → movement, `F`
  → despawn the moon, wheel-vertical → scroll) — and the `--actions` summary
  reports `actions: {fixed} fixed steps, {moved} moved, {wheel} wheel ticks`.
- **The measurement is honest**: `scripts/bench_input.sh` runs the same batch
  stream through the engine step, the C backend alone, and the Rust reference;
  the recorded result is that **the C backend is slower than the Rust oracle**
  (≈16 vs ≈11 ns/event; the full engine step ≈36 ns/event), and no speedup is
  claimed. A Valgrind run of the benchmark binary reports **0 errors and 0
  bytes lost**.
- **The decision is explicit**: the C backend is the low-level device
  foundation's assigned home and the context-arbitration state machine is C++'s
  (ADR 0013) — the driver is architectural, not a profiled bottleneck.
- **End-to-end verification**: `scripts/verify_input_actions.sh` — holding `D`
  for 1 s moves the square to the right wall through action state; two wheel
  notches appear as `wheel ticks ≥ 2` in the `--actions` summary; a tapped
  `Escape` quits with exit 0 through the action-mapping press edge with the
  same log line as the raw lane. Wired into `scripts/verify.sh` and the CI
  smoke job.

## Milestone: the audio system (WAV assets, a miniaudio-backed device, a C++ mixer) — complete (owner-directed roadmap amendment)

The layer that turns decoded sample data into sound on a device and into
deterministic blocks everywhere else. The roadmap did not define audio as
Phase 9; the project owner directed the milestone, so it arrives through ADR
0014 the same way Luau scripting did through ADR 0011. It is the fourth
first-party native module and the first one whose low-level C layer owns an
operating-system resource (an output device) rather than arithmetic over
caller-owned data:

- **`mikan-audio-sys`**, the fourth crate allowed to use `unsafe`: a C device
  backend (`src/audio_backend.c` — the miniaudio context and device, the
  realtime output callback, the deterministic offline ring with
  `pump`/`read_s16`/overruns, and compiler-atomic helpers because the C99
  sources cannot use C11 atomics) next to a hand-written C++ mixer runtime
  (`src/audio_runtime.cpp` — 256 sound slots and 64 voices in fixed pools, f32
  stereo mixing with linear-interpolation resampling, bus/master gain atoms, a
  256-command SPSC queue drained at block boundaries, generation-checked sound
  *and* voice handles, and two-phase sound removal that stops orphaned voices).
  The two layers cross exactly one function (`mikan_audio_runtime_render`) and
  share no types; warnings-as-errors and `-fno-exceptions -fno-rtti` come from
  the same `build.rs` discipline as the earlier modules.
- **Vendored miniaudio 0.11.25** (`third_party/miniaudio/miniaudio.h`, SHA-256
  recorded next to it) — the "existing library consumed on its own terms" case
  of `docs/ffi.md`. Its silent "null" backend is **disabled in the vendored
  config**: a machine with no device must report `NO_DEVICE` and let the caller
  choose the offline backend, never pretend to play.
- **`mikan-audio`**, the safe face (no `unsafe`): `AudioSystem` (device via
  `new_device`, deterministic offline via `new_offline`; `update()` is a no-op
  on devices and one pump + `collect` offline), `Sound`/`Voice`
  (generation-checked copyable handles), `PlaySettings`, `Bus {Sfx, Music}`,
  and `AudioError` mapping every native status
  (`NoDevice/DeviceError/InvalidSampleRate/InvalidArgument/Unsupported/
  Capacity/NotFound/Busy/OutOfMemory/OfflineOnly/Asset`). `AudioSystem` is
  deliberately neither `Send` nor `Sync`: the native contract is one control
  thread plus the render side.
- **`mikan-assets` grew the decoder**: `SoundData` + `decode_wav`/`load_wav`
  over a strict RIFF/WAVE little-endian subset — PCM 8/16/24/32-bit and IEEE
  float 32/64, mono/stereo, 8 000–192 000 Hz, `WAVE_FORMAT_EXTENSIBLE` with the
  standard GUIDs, unknown chunks skipped with padding honored, a `2^24`-frame
  cap with a fallible reservation, and a specific error for every rejection
  (ADPCM, >2 channels, 12/20-bit layouts, truncated or inconsistent chunks)
  with **no partial decode** ever returned.
- **Cross-checks are tests, not prose**: the C++ mixer's output is compared
  **bit for bit** in the s16 domain against an independent Rust mirror in
  `mikan-audio-sys::reference`, on exact dyadic inputs, so any arithmetic
  divergence is detectable; the backend, the runtime, and the raw FFI each
  have their own suites, and the safe crate's tests run on the offline backend
  and on the device path's documented `NO_DEVICE` outcome. No test requires a
  sound card.
- **The demo integrates it**: `demo2d --audio` opens the device and falls back
  to the offline backend with an explicit warning when it cannot start, loads
  `assets/blip.wav` through the real pipeline, plays it on every edge from
  standing to moving (pitch 1.25 up, 0.75 down, 1.0 otherwise), drains the
  offline ring once per displayed frame, and summarizes at shutdown:
  `audio: {plays} plays, {started} voices started, {finished} finished,
  {blocks} blocks mixed, {overruns} overruns, backend {name}`.
- **The blip is a generated asset**: `scripts/generate_assets.py` now writes
  `blip.wav` (5760 frames, 48 kHz, mono, 16-bit: a 1320→660 Hz linear sweep
  with a 4 ms attack and a quadratic fade) deterministically, and
  `--check` covers it like the PNGs and the sheet JSON.
- **The measurement is honest**: `scripts/bench_audio.sh` times the mixer
  through the safe API on the offline backend — `update()` + a full
  `drain_samples()` per block — with 16 timed runs over 4096 × 1024-frame
  blocks after 3 warm-ups. Recorded on this machine (Intel Core i3-N305, 8
  threads, release mode, `artifacts/bench/audio_4096.log`): **12.2 ns/frame**
  with 1 looping voice, **62.6** with 16, **198.8** with the full 64-voice
  pool, and **192.4** with 64 voices at pitch 1.25 (the resampling path). The
  realtime budget at 48 kHz is 20 833 ns/frame, so even the worst case the
  pool allows uses under 1 % of one core and runs 105× faster than realtime.
  No speedup over a Rust baseline is claimed (ADR 0014: the driver is
  architectural).
- **The decision is explicit**: C owns the device foundation (contexts,
  buffers, the output callback, the offline ring) and C++ owns the playback
  runtime and mixer state machine (ADR 0014) — the same split of duties as
  collision, scene, and input.
- **End-to-end verification**: `scripts/verify_audio.sh` — the lane starts and
  logs its backend, `blip.wav` decodes as 5760 frames at 48 kHz mono, `D` then
  `W` start exactly one voice each with the second at the up pitch (`#1 at
  pitch 1.00`, `#2 at pitch 1.25`), both voices finish, the offline ring
  reports 0 overruns, Escape quits with status 0, a second run with one key
  press reports exactly one play, and any `audio: could not` line fails the
  run. Wired into `scripts/verify.sh` and the CI smoke job. Local run:
  `backend offline: 2 plays, 2 started, 2 finished, 425 blocks mixed, 0
  overruns`, then `1 plays, 1 voices started`.
- **Valgrind: not yet run.** This machine has no Valgrind and installing it
  needs interactive authorization, so `docs/STATUS.md` records the exact
  command and the pending status instead of a result — the one item of the
  `docs/ffi.md` checklist this milestone does not close yet.

## What is implemented

| Area | What exists | Coverage |
| --- | --- | --- |
| `mikan-core` | `EngineConfig` + validation, `Color`, `Time` (clamped delta, zero first tick), `FixedTimestep` (bounded catch-up, interpolation alpha), `FrameOutcome`, `Camera2d` (position, zoom, `world_to_screen`, sanitizing `effective_zoom`, `pixel_space`, `pixel_perfect` integer zoom + snap, `pixel_perfect_scale`, `snap_to_physical_pixels`), `UvRect` (normalized region, collapsible half-texel `inset`), one-shot `init_logging` | 32 unit tests |
| `mikan-assets` | `Image` (PNG → RGBA8, all color types normalized, `MAX_IMAGE_DIMENSION` + checked allocation, `ImageError`), `SpriteSheet`/`SheetFrame` (Aseprite JSON subset, `meta.image` resolution, duplicate detection via a custom serde visitor, geometry/pivot validation, rotated/trimmed rejection, inset `uv`, `center_for_pivot`), `SheetError`, `SoundData` + strict WAV decoding (`decode_wav`/`load_wav`, PCM 8/16/24/32 + float 32/64, mono/stereo, 8–192 kHz, `WAVE_FORMAT_EXTENSIBLE`, a `2^24`-frame cap, no partial decodes, `SoundError`) | 59 unit tests, 1 doc test |
| `mikan-audio-sys` | the C device backend (`mikan_audio_backend`: miniaudio context/device init, the realtime output callback wired to the runtime, the deterministic offline ring with `pump`/`read_s16`/overruns, atomic helpers for C99) and the hand-written C++ mixer (`mikan_audio_runtime`: 256 sound slots, 64 voices, f32 stereo mixing with linear-interpolation resampling, bus/master gain atoms, a 256-command SPSC queue drained at block boundaries, generation-checked sound and voice handles, two-phase sound removal); vendored miniaudio 0.11.25 with its hash recorded and the null backend disabled; warnings-as-errors and `-fno-exceptions -fno-rtti` `build.rs`; the fourth crate allowed `unsafe` (override and reason recorded in its crate root) | 24 integration tests (6 backend, 10 runtime, 3 raw FFI, 5 bit-for-bit cross-checks against the Rust reference) |
| `mikan-audio` | `AudioSystem` (device via `new_device`, deterministic offline via `new_offline`, `update()` a no-op on devices), `Sound`/`Voice` generation-checked handles, `PlaySettings`/`Bus`, `load_wav` through the asset pipeline, `drain_samples`/`overruns` (offline-only, enforced), and `AudioError` mapping every native status; no `unsafe`, not `Send`/`Sync` by contract | 8 unit tests, 1 doc test |
| `mikan-ecs` | `Entity` (generational handle, `Display` `{index}v{generation}`, LIFO slot reuse, stale-handle rejection), `World` (insertion-ordered sparse-set storage per component type, typed resources, `iter`/`iter_mut`/`iter2`/`iter2_mut`, `component_count`, `entity_count`, `Debug`), `Schedule` (named systems once each in registration order, `names`/`len`/`clear`), `DeadEntity` | 42 unit tests, 5 integration tests (determinism), 4 doc tests |
| `mikan-collision-sys` | the C99 narrow-phase kernel (`mikan_contacts` over plain `#[repr(C)]` data, warnings-as-errors `build.rs`), the raw binding with `# Safety` docs, and a safe batch wrapper with debug assertions on pair indices; one of the two crates allowed `unsafe` (override and reason recorded in its crate root) | 7 integration tests (kernel semantics, boundary and degenerate cases, index validation) |
| `mikan-collision` | `Aabb`/`Pair`/`Contact` with documented semantics, uniform-grid `Broadphase` (sorted, deduplicated, deterministic output), `contacts`, and `reference` — the Rust oracle and benchmark baseline, cross-checked bit for bit (including a randomized 256-box scene stepped 8 × 20,000 times) | 6 broad-phase integration tests, 3 cross-check integration tests, 2 doc tests |
| `mikan-scene-sys` | the C compose kernel (`mikan_transform2d_compose` over a seven-`f32` `#[repr(C)]` struct, `-ffp-contract=off`) and the hand-written C++ scene graph (one opaque handle, `extern "C"` entry points, status codes, warnings-as-errors and `-fno-exceptions -fno-rtti` `build.rs`); the second crate allowed `unsafe` (override and reason recorded in its crate root) | 6 unit tests, 8 integration tests (graph semantics, error paths, removal-reroots invariant, propagate ordering, bit-exact compose) |
| `mikan-scene` | `Transform2d`/`GlobalTransform2d`, `Hierarchy` (forest invariants, re-root on removal), `DespawnQueue`, `Scene` (one world + schedule, ordered update), `SceneManager` (generational ids, one active scene), `propagate_transforms`, and `reference` — the Rust oracle, cross-checked bit for bit | 17 unit tests, 11 integration tests (scene behavior), 4 cross-check integration tests, 4 doc tests |
| `mikan-script` | `ScriptEngine` (`from_file`/`from_source`, `call_step`, `name`, `used_memory`), `StepInput`/`StepOutput`, and `ScriptError` (`Load`, `Compile`, `MissingFunction`, `Runtime`, `InvalidReturn`, `Vm`) carrying the script name and line; explicit safe library set, Luau `sandbox(true)`, 64 MiB memory limit; vendored Luau through `mlua` 0.11.6 | 18 unit tests |
| `mikan-platform` | `App` lifecycle trait, `AppContext` (input borrowed `&` for the raw lane, `&mut` for the event log), `InputState` (held state, press/release edges, cursor delta, focus release), the per-frame `DeviceEvent` log (`take_events` drain, wheel normalization, bounded drop-and-count), `winit` runner with occlusion/suspension-aware waiting | 14 unit tests, 1 doc test |
| `mikan-input-sys` | the C device backend (`mikan_input_backend`: canonical events over USB HID usage IDs, raw device state, press/release/repeat normalization, move/wheel coalescing, synthetic releases on focus loss, bounded buffer) and the hand-written C++ action runtime (`mikan_input_runtime`: prioritized contexts, per-event arbitration, action state machines); warnings-as-errors and `-fno-exceptions -fno-rtti` `build.rs`; the third crate allowed `unsafe` (override and reason recorded in its crate root) | 4 unit tests, 29 integration tests (backend semantics, runtime behavior, error paths, bit-exact cross-check against the Rust normalizer) |
| `mikan-input` | `InputSystem` (owns backend + runtime, translates `DeviceEvent`s, maps statuses to `InputError`), `ActionId`/`ContextId`, `Binding`/`WheelAxis`, the winit → HID code `map`, and `reference` — the independent Rust normalizer oracle, cross-checked bit for bit | 3 unit tests (code/button mapping), 16 integration tests (edges, arbitration, wheel values, focus, lifecycle), 1 doc test |
| `mikan` | the `Game` lifecycle over the platform `App` (renderer created before `init`, resizes forwarded before the game sees them, `shutdown` iff `init` ran), the `Engine` handle (`AppContext` + `&Renderer` + `render_frame`), labeled renderer-startup failures, curated re-exports of all ten engine crates (including `mikan_ecs`, `mikan_collision`, `mikan_scene`, `mikan_script`, `mikan_input`, and `mikan_audio` with their common types) | 3 unit tests, 2 doc tests |
| `mikan-render` | `wgpu` instance/adapter/device/queue/surface, format/alpha/present-mode selection, explicit handling of every swap chain state, resize (including zero-sized), device-lost reporting, instanced sprite pipeline (flat or textured, per-sprite rotation, tint, alpha, linear/nearest sampling as a batch key), texture upload with declared encoding (`Linear`/`Srgb`, validated), camera view/projection matrix (including pixel-perfect), `FrameStats { sprites, batches, draw_calls }`, RGBA8 capture readback | 27 unit tests, 1 doc test |
| `games/demo2d` | built on the `mikan` facade: CLI (`--frames`, `--capture`, `--stats`, `--timeout`, `--scene`, `--sampler`, `--pixel-perfect`, `--assets`, `--size`, `--camera`, `--zoom`, `--no-vsync`, `--sprites`, `--script`, `--actions`, `--audio`, `--help`), four scenes, asset loading/upload/validation, the classic scene as a `mikan-ecs` world (player plus three static collision blocks, `collide` resolution, five ordered systems, interpolated rendering), the hierarchy scene as a `mikan-scene` world (sun, planets, moon, spin, deferred despawn), a Luau gameplay script driving movement (load errors fatal, runtime errors logged once with an idle fallback), the `--actions` lane (an `ActionMapping` over `mikan-input`: global Escape-quit context above a WASD/arrows/`F`/wheel gameplay context, the shutdown summary `{fixed} fixed steps, {moved} moved, {wheel} wheel ticks`), the `--audio` lane (device with an explicit offline fallback, `blip.wav` through the asset pipeline, a blip on every movement edge with pitch following the vertical direction, a drain per displayed frame, and the shutdown summary `{plays} plays, … overruns, backend {name}`), PPM writer, frame-time and draw statistics | 56 unit tests |

## Verification: build and tests (this revision, local)

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo check --workspace --all-targets` | pass |
| `cargo test --workspace` | **421 pass, 0 fail** — 291 unit tests (60 mikan-assets, 56 demo2d, 42 mikan-ecs, 32 mikan-core, 27 mikan-render, 18 mikan-script, 17 mikan-scene, 14 mikan-platform, 9 mikan-audio, 6 mikan-scene-sys, 4 mikan-input-sys, 3 mikan, 3 mikan-input), 113 integration tests (29 mikan-input-sys — 12 backend, 15 runtime, 2 cross-check — 24 mikan-audio-sys — 6 backend, 10 runtime, 3 raw FFI, 5 cross-check — 16 mikan-input, 15 mikan-scene — 11 scene, 4 cross-check — 9 mikan-collision — 6 broad-phase, 3 cross-check — 8 mikan-scene-sys, 7 mikan-collision-sys, 5 mikan-ecs determinism), and 17 doc tests (4 mikan-scene, 4 mikan-ecs, 2 mikan, 2 mikan-collision, 1 mikan-assets, 1 mikan-audio, 1 mikan-input, 1 mikan-platform, 1 mikan-render) |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo build --workspace --release` | pass |
| `python3 scripts/generate_assets.py --check` | pass: atlas.png, star.png, atlas.json, blip.wav all match the generator |

`./scripts/verify.sh` runs all of the above plus the runtime checks below.

## Verification: runtime (Linux/X11, software Vulkan)

Environment: Xvfb virtual display, Mesa `lavapipe` software Vulkan device
(`llvmpipe (LLVM 20.1.2, 256 bits)`, device type CPU), window 960x600, surface
format `Bgra8UnormSrgb`.

| Check | Command | Observed result |
| --- | --- | --- |
| Frame presentation + capture (classic scene) | `xvfb-run -a ./target/release/demo2d --frames 120 --capture artifacts/frame.ppm --stats` | 120 of 120 frames presented, exit 0; `assets: games/demo2d/assets -> atlas 96x32 (3 frames), star 24x24` |
| Captured image contents | `python3 scripts/verify_capture.py artifacts/frame.ppm` | background `(63, 69, 80)`; player center `(253, 196, 108)` — the exact byte stored in `atlas.png`, which the sRGB upload path now preserves |
| Texture sampling | pixel probes of the same capture | player border `(179, 129, 63)`; marker center `(160, 225, 249)` and border `(89, 149, 188)`: both atlas frames are sampled, not flat colors |
| Draw statistics (classic) | `--stats` line | `sprites 5 | batches 2 | draw calls 2` (three blocks, the player, and the marker) |
| Keyboard movement | `xvfb-run -a bash scripts/verify_input.sh` | holding `D` for 1 s moved the square to world `[912, 300]` (the 912-pixel wall; earlier runs land anywhere in 730..912 depending on keyup timing); a tapped `Escape` exited 0 before the frame budget |
| Action-mapping movement | `xvfb-run -a bash scripts/verify_input_actions.sh` | holding `D` for 1 s under `--actions` moved the square through action state to the 912-pixel wall `[912, 300]` |
| Action-mapping wheel | as above | two `click 4` wheel notches appear in the shutdown summary as `actions: {fixed} fixed steps, {moved} moved, {wheel} wheel ticks` with wheel ticks ≥ 2 |
| Action-mapping quit | as above | a tapped `Escape` exits 0 with the same `escape pressed` log line as the raw lane, the log shows `input: action mapping active`, and no frame-budget timeout fires |
| Audio lane startup | `xvfb-run -a bash scripts/verify_audio.sh` | this machine has no sound card, so the lane logs `audio: no usable output device (…); using the offline backend at 48000 Hz, block 1024 frames`, then `audio: games/demo2d/assets/blip.wav -> 5760 frames at 48000 Hz, 1 channel(s)` and `audio: the blip plays when movement starts (pitch follows up/down)` |
| Audio playback edges | as above | holding `D` then `W` (0.6 s each, with a release between): `audio: blip #1 at pitch 1.00` and `audio: blip #2 at pitch 1.25` |
| Audio counters and clean exit | as above | the shutdown summary reports `backend offline: 2 plays, 2 started, 2 finished, 425 blocks mixed, 0 overruns`; Escape exits 0 with `escape pressed`; no `audio: could not` line in the log; a second run with one `A` press reports exactly `1 plays, 1 voices started` |
| ECS schedule (startup) | `xvfb-run -a bash scripts/verify_ecs.sh` | startup log: `ecs: 4 entities, 11 components, systems: snapshot, input, movement, collide, clamp` |
| ECS fixed-step integration | as above | holding `D` for ~0.5 s: `moved steps: 65 of 175 fixed steps; square at [708.0, 300.0]` — 480 + 65 × 3.5 = 707.5 rounds to 708, so every moved step was exactly one 3.5 px fixed step |
| ECS no-input run | as above | no input for 120 frames: `0 moved steps` and `square at [480, 300]` (the fixed-step count varies with run speed); nothing moves without input |
| Collision startup facts | smoke run log | `collision: 3 static blocks, grid cell 160` |
| Collision resolution (diagonal into the first pillar) | `xvfb-run -a bash scripts/verify_collision.sh` | holding down+right: `square ended at [582.0, 552.0]` — stopped at the first pillar's left face (x = 630 − 48 = 582) and slid down it to the floor clamp (y = 552) |
| Collision corridor | as above | a straight run along y = 300 ended at `[910.0, 300.0]` (the right wall): the blocks leave the documented corridor unobstructed |
| Scripting startup fact | smoke run log | `script: games/demo2d/scripts/player.luau (mikan_step)` |
| Scripting: the default script drives movement | `xvfb-run -a bash scripts/verify_script.sh` | holding `D`: `moved steps: 63 of 173 fixed steps; square at [700.0, 300.0]` — 480 + 63 × 3.5 = 700.5 rounds to 700, one 3.5 px fixed step per moved step |
| Scripting: over-long directions are normalized | as above | a `(3, 4, 60)` return for exactly 60 steps: `moved steps: 60 of 430; square at [498.0, 324.0]` — the normalized `(0.6, 0.8)` path; unnormalized it would be `[570, 420]`, and a broken step counter would clamp at `[912, 552]` |
| Scripting: failure paths | as above | syntax error, missing `mikan_step`, and missing file: exit 1 with the script path and line (`failed to load: syntax error: ...:2:`, `does not define a global function \`mikan_step\``, `could not read script ...`); a runtime `error("boom")` and a `0/0` return: exit 0, exactly one `script error:` line each, `0 moved steps` |
| Scene startup layout | `xvfb-run -a bash scripts/verify_scene.sh` | log: `hierarchy: sun ... + [...] + moon ... | orbit 160, moon orbit 80`; capture: sun warm `(253, 196, 108)` at `[480, 300]`, first planet blue `(160, 225, 249)` at `[640, 300]`, moon star `(255, 243, 124)` at `[720, 300]` — the local-translation chain composed correctly |
| Scene rigid rotation | as above (holding `D`) | the moon's centroid stays `239.2 px` from the sun (one orbit radius, 240 px) while its angle changes by `44.1°` — rigid rotation, not a broken scale/rotate/translate order |
| Scene deferred despawn | as above (pressing `F`) | `0` star pixels remain and `5164` planet pixels remain: the moon alone was despawned through the queue, and the sun and planets stayed |
| Follow camera | `xvfb-run -a bash scripts/verify_camera.sh` | world position `[912, 300]` while the capture shows the square 96x96 px at `[432..527, 252..347]`, centroid `(480.8, 300.8)` — centred, so the view scrolled |
| Zoom | as above (`--camera follow --zoom 2`) | the square renders 192x192 px at `[384..575, 204..395]`, centroid `(482.1, 302.1)` — twice the unzoomed size |
| Showcase scene (rotation, tint, alpha, samplers, PNG alpha) | `xvfb-run -a bash scripts/verify_showcase.sh` | `sprites 8 | batches 4 | draw calls 4`; plain center `(253, 196, 108)`; rotated box corner clear `(63, 69, 80)` while the diamond tip paints `(179, 129, 63)` above the unrotated box; tinted `(136, 156, 108)`; alpha-blended `(174, 138, 93)`; linear checker scanline: 18 distinct grays, 160/160 intermediate; nearest: black/white only; star center `(255, 243, 124)` with backdrop `(80, 97, 149)` through its corners |
| Stress batching (1000 sprites) | `xvfb-run -a bash scripts/verify_stress.sh` | `stress: 1000 sprites, 24x24 cells`; `sprites 1000 | batches 1 | draw calls 1`; 37.0% of pixels are the player tile's warm colors, 38.7% non-clear, 132,983 exactly base color |
| Pixel-perfect mode | `xvfb-run -a bash scripts/verify_pixel_perfect.sh` | `pixel-perfect: virtual 240x150, integer zoom x4`; the player square is exactly 384x384 px at `[288..671, 108..491]` — every edge on a multiple of 4, centre `(480.0, 300.0)` |
| Asset failure modes | `xvfb-run -a bash scripts/verify_assets.sh` | missing directory → `could not read`; corrupt PNG → `could not decode PNG data`; sheet without `player` → `has no frame "player"`; all exit non-zero before presenting a frame |
| Frame budget timeout | `--frames 100000000 --timeout 1` | exit 1: `presented 376 of 100000000 requested frames within 1s` (the count varies with machine load) |
| Unwritable capture path | `--frames 5 --capture /definitely/not/a/dir/f.ppm` | exit 1: `could not write '/definitely/not/a/dir/f.ppm': No such file or directory (os error 2)` |
| Invalid argument / option combination | `--nope`; `--zoom 2`; `--sprites 100`; `--camera follow --zoom 2 --pixel-perfect 320x200` | exit 1 with `unknown argument '--nope'`, `'--zoom' requires '--camera' (for example: --camera follow --zoom 2)`, `'--sprites' only applies to '--scene stress'`, `'--zoom' cannot be combined with '--pixel-perfect': the integer zoom is derived from the virtual resolution` |
| Help | `--help` | exit 0, usage printed (all options and scenes listed) |

Captured classic-scene values are identical to the previous milestone's expected
values, except that the player center is now the PNG byte itself
(`(253, 196, 108)` vs `(253, 195, 108)` before), because the sRGB texture path
replaced the CPU conversion.

## Verification: 10,000-sprite benchmark (this machine, software rasterizer)

`xvfb-run -a bash scripts/bench_sprites.sh "100 1000 10000" 300`, recorded in
`artifacts/bench/`; all runs report **1 batch, 1 draw call** for their sprite
count:

| Window | Sprites | frames | mean ms | p50 ms | p95 ms | max ms | mean fps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 320x200 | 100 | 300 | 1.21 | 1.03 | 1.94 | 10.66 | 823.3 |
| 320x200 | 1000 | 300 | 6.05 | 5.54 | 9.01 | 15.27 | 165.3 |
| 320x200 | 10000 | 300 | 60.72 | 55.63 | 90.60 | 141.94 | 16.5 |
| 960x600 | 100 | 300 | 2.60 | 2.26 | 4.63 | 13.14 | 384.6 |
| 960x600 | 1000 | 300 | 11.79 | 10.70 | 21.99 | 32.11 | 84.8 |
| 960x600 | 10000 | 300 | 71.26 | 65.16 | 111.66 | 243.96 | 14.0 |

How to read this, honestly:

- These numbers describe **`llvmpipe` on one machine** with no real vsync;
  they are not representative of GPU performance and must not be quoted as if
  they were.
- Batching is not the bottleneck: every run is one draw call. The 10,000-sprite
  cost is dominated by rasterization: each sprite is 16x16 px, so a frame
  shades ~2.56M sprite fragments regardless of window size (which is why the
  two window sizes differ by ~10 ms at 10,000 sprites, not by 9x), and this
  software rasterizer sustains roughly 36M sprite fragments per second.
- **The 60 FPS target is met up to 1,000 sprites** (84.8 fps at 960x600) **and
  not at 10,000 on this device** (14.0 fps). The target is a measurable goal,
  not a guarantee; a hardware GPU has not been measured (see limitations).

## Verification: the collision kernel benchmark (this machine, CPU only)

`bash scripts/bench_collision.sh` runs 250,000 candidate pairs generated from
512 boxes (14,041 of them overlapping) through the C kernel and the Rust
reference, 25 runs each, in release mode; the result is recorded in
`artifacts/bench/collision_250000.log`. Three runs of the same script on this
machine:

| Run | C kernel (ns/pair) | Rust reference (ns/pair) | C/Rust |
| --- | --- | --- | --- |
| 1 | 8.55 | 12.67 | 0.68 |
| 2 | 10.02 | 9.65 | 1.04 |
| 3 (recorded this revision) | 12.05 | 11.08 | 1.09 |

The winner flips between runs and both implementations land in the 8–13
ns/pair band: **parity within run-to-run noise** (below 1.0 means C faster).
No speedup is claimed — the kernel exists because the low-level numeric layer
is C's assigned home (ADR 0010), not because it beat Rust, and this table is
the honest measurement the ADR demands.

Valgrind (memcheck) over a full run of the same binary:

```
valgrind --leak-check=full --error-exitcode=1 ./target/release/examples/bench 250000
```

exit 0; `ERROR SUMMARY: 0 errors from 0 contexts (suppressed: 0 from 0)`;
0 bytes definitely or indirectly lost (548 bytes still reachable in 2 blocks —
Rust runtime internals). This is the memory-tool record `docs/ffi.md` requires
for a native module.

## Verification: the scene hierarchy benchmark (this machine, CPU only)

`bash scripts/bench_scene.sh` runs a 100,000-node binary-heap hierarchy through
the native scene graph (the C++ topology driving the C compose kernel) and
through the Rust reference fold (`mikan_scene::reference`), 15 runs each after
warm-up, in release mode; the result is recorded in
`artifacts/bench/scene_100000.log`:

| Implementation | ms/run | ns/node |
| --- | --- | --- |
| native graph (C++ + C) | 10.633 | 106.33 |
| Rust reference fold | 0.704 | 7.04 |
| ratio (native / reference) | **15.11** | — |

The native graph is **markedly slower**, which is the expected and reported
result: the reference gets the topology for free from a precomputed parent array,
while the native graph carries real bookkeeping (an `unordered_map` index and
parent lookups plus per-call traversal buffers). The compose kernel itself is at
parity — the cross-check tests prove the two implementations agree bit for bit —
so the gap is the graph plumbing, not the arithmetic. No speedup is claimed; the
kernel exists because the low-level numeric layer is C's assigned home and a
dynamic topology is C++'s, per ADR 0012. Caching the index/topology and moving
to a `Vec`-backed node table are the obvious future work if a scene ever becomes
hot.

Valgrind (memcheck) over a full run of the benchmark binary:

```
valgrind --leak-check=full --error-exitcode=1 ./target/release/examples/bench_scene 20000
```

exit 0; `ERROR SUMMARY: 0 errors from 0 contexts (suppressed: 0 from 0)`;
0 bytes definitely, indirectly, or possibly lost (548 bytes still reachable in 2
blocks — Rust runtime internals, the same figure the collision kernel reports).
The native-to-reference ratio under Valgrind is 10.37 — lower than the native
run because the tool instruments both sides. This is the memory-tool record
`docs/ffi.md` requires for a native module.

## Verification: the input benchmark (this machine, CPU only)

`bash scripts/bench_input.sh` runs the same deterministic batch stream — 6 key
transitions, 2 cursor moves, 1 wheel tick per batch (9 events) — through the
engine step (`InputSystem::process`: Rust translation + the C backend + the C++
action runtime, then queries and `end_step`), the C backend alone (same pushes
+ drain), and the Rust `Normalizer` oracle, 40 runs each after 3 warm-ups, in
release mode, with a bit-for-bit parity gate between the two normalizers before
any timing is printed; the result is recorded in
`artifacts/bench/input_65536.log` (65 536 batches):

| Implementation | ns/batch | ns/event | batches/s |
| --- | --- | --- | --- |
| engine step (translation + backend + runtime) | 326.3 | 36.25 | ≈3.06M |
| native backend (C, kernel only) | 143.5 | 15.94 | ≈6.97M |
| Rust reference (Normalizer oracle) | 96.6 | 10.73 | ≈10.36M |

The honest reading: **the C backend measures slower than the Rust oracle**
(≈15.94 vs ≈10.73 ns/event — about 1.5×), and the translation/runtime
wrapper adds the remaining gap to the engine step, which stays under
≈40 ns/event end to end. As with collision and scene, this is the expected
and reported result: the input module exists because the low-level device
foundation is C's assigned home and the action state machine is C++'s (ADR
0013), not because it beat Rust — and the parity gate proves the two
normalizations agree bit for bit, which is the module's real contract.

Valgrind (memcheck) over a full run of the benchmark binary:

```
valgrind --leak-check=full --error-exitcode=1 --log-file=artifacts/mem/input_bench_valgrind.log target/release/examples/bench_input 16384
```

exit 0; `ERROR SUMMARY: 0 errors from 0 contexts (suppressed: 0 from 0)`;
0 bytes definitely, indirectly, or possibly lost (548 bytes still reachable in
2 blocks — Rust runtime internals, the same figure the collision and scene
benchmarks report). This is the memory-tool record `docs/ffi.md` requires for
a native module.

## Verification: the audio mixer benchmark (this machine, CPU only)

`bash scripts/bench_audio.sh` runs the mixer (the C++ runtime through
`mikan-audio`'s safe API) on the deterministic **offline backend**, 16 timed
runs over 4096 blocks × 1024 frames (about 87 s of audio per run) after 3
warm-ups, in release mode, measuring `update()` + a full `drain_samples()` per
block: mix, hard clip, ring store, and the s16 conversion. The result is
recorded in `artifacts/bench/audio_4096.log`:

| Row | ns/frame | frames/s | vs. realtime |
| --- | --- | --- | --- |
| 1 looping voice, pitch 1.0 | 12.2 | ≈82.3M | 1714× |
| 16 looping voices, pitch 1.0 | 62.6 | ≈16.0M | 333× |
| 64 looping voices, pitch 1.0 (full pool) | 198.8 | ≈5.0M | 105× |
| 64 looping voices, pitch 1.25 (resampling) | 192.4 | ≈5.2M | 108× |

The honest reading: **no speedup over a Rust baseline is claimed or needed** —
the driver for this module is the architecture directive (ADR 0014), and the
numbers answer the practical question instead: the worst case the pool allows
(64 summed, clipping voices) costs under 1 % of one core's realtime budget at
48 kHz (20 833 ns/frame), so the audio thread has three orders of magnitude of
headroom on this machine. The measured region includes the ring store and the
s16 conversion, which on the device path correspond to the backend's copy,
so the callback is not slower than the recorded row.

Valgrind (memcheck) has **not** been run for this module yet: this machine has
no `valgrind`, and installing it requires interactive `sudo` (unavailable in
this environment). The exact command to run, kept here so the record can be
completed later:

```
valgrind --leak-check=full --error-exitcode=1 --log-file=artifacts/mem/audio_bench_valgrind.log target/release/examples/bench_audio 2048
```

Until that run happens, this milestone does not claim the Valgrind item of
`docs/ffi.md`'s checklist — the pending status is the honest record.

## Verification: CI

**The full pushed history is covered, and every milestone's final commit is
green.** The latest pushed commit, `99fcbb6` (the Wayland hardening of the
input verify scripts; run
[39029252348](https://github.com/neverseeyouagainnnnn/mikan/actions/runs/39029252348)),
and the Phase 8 milestone commit `23be31e` (run
[39025956812](https://github.com/neverseeyouagainnnnn/mikan/actions/runs/39025956812))
pass all four jobs: Quality (formatting, types, tests, clippy, the release
build, and the generator check), the headless smoke test (the demo, the
captured-frame check, and the input, action-mapping, camera, ecs, collision,
scripting, scene, showcase, stress, pixel-perfect, and asset verifications),
and the Windows and macOS compile checks — the first CI compiles of the C
collision kernel, the C transform kernel and C++ scene graph, the C input
backend and C++ action runtime, and the vendored Luau C++. Two earlier pushes
were red and are recorded as such: `860d698` (the first scene/entity commit;
the macOS compile check failed, fixed in `07c6c33`) and `773589e` (the first
input commit; `cargo fmt --check` failed, fixed in `23be31e`). Phase 5's
`5f63579` run was cancelled when a later push superseded it. The earlier
facade-milestone state `9c3e98b` remains green in run
[38753924627](https://github.com/neverseeyouagainnnnn/mikan/actions/runs/38753924627).

**This revision (the audio milestone) is unpushed, so no CI run covers it
yet.** The workflow's smoke job now also runs
`xvfb-run -a bash scripts/verify_audio.sh` (on the runners, which have no sound
card, the lane exercises the documented `NO_DEVICE` fallback and the offline
mixer path — the same one this machine verified locally), and the quality
job's `cargo test --workspace` includes every new audio test plus
`generate_assets.py --check` over the new `blip.wav`. The first push of this
change set is the point at which those claims become CI-verified; until then
they are local-run claims, listed as such above.

## Verification levels by platform (ADR 0005)

| Platform | Level | Evidence |
| --- | --- | --- |
| Linux / X11 (software Vulkan) | **runtime-verified** | local smoke, input (raw lane and action-mapping lane), audio (device attempt + offline mixer, blip through the asset pipeline, counters), camera, ecs, collision, scripting, scene, showcase, stress, pixel-perfect, and asset-failure checks; the CI smoke job runs the same verifications plus `verify_audio.sh` (input-era runs 39029252348/39025956812; the audio run is pending the first push) |
| Windows x86-64 | **compile-verified** for the pushed milestones | CI `cargo check --workspace --all-targets` builds every crate, including the native modules and vendored miniaudio, on windows-latest (run 39025956812 for the input milestone; the audio change set is unpushed) |
| macOS (arm64) | **compile-verified** for the pushed milestones | as above, on macos-latest (run 39025956812 for the input milestone; the audio change set is unpushed) |
| Linux / Wayland | not verified | path exists through `winit`; never exercised |
| Hardware GPU / Windows / macOS runtime | not verified | no such machine in the verification environment |
| Any machine with a real sound card | **not verified** | this machine has no ALSA/PulseAudio/JACK and no `/dev/snd`, so the device path has only been exercised to its documented `NO_DEVICE` outcome; the mixer it would run is the same one the offline backend runs |

## Known limitations

- **Graphics scope:** sprites are rectangles with per-sprite rotation, tint,
  and alpha, sampled linearly or nearest; there are no mipmaps, no texture
  arrays, and no render-to-texture. Textures are single-mip RGBA8 with a
  declared encoding; PNGs are the only decoded format. Batching merges only
  consecutive sprites sharing a texture *and* sampler and never reorders them.
- **Asset scope:** `mikan-assets` decodes PNG files and sheet JSON; there is no
  atlas packing, no hot reload, no background loading, and only the first frame
  of an animated PNG is decoded. `gAMA`/`cHRM` chunks are ignored: file bytes
  are treated as sRGB. Rotated and trimmed sheet frames are rejected.
- **Camera scope:** position and zoom (`pixel_space`, `pixel_perfect`, or a
  follow camera is game code) — no rotation, bounds, smoothing, or parallax.
- **Pixel-perfect scope (ADR 0008):** the virtual screen is centered to within
  one physical pixel with no letterbox fill; only the camera is snapped — a
  game that wants sprites on the virtual grid must round their positions.
- **Facade scope:** one window and one renderer per process; multi-window
  applications must use `mikan_platform::App` directly. `Game::Error` must
  convert from `RenderError`.
- **ECS scope:** `mikan-ecs` is deliberately lite: single-threaded by design
  (no `Send`/`Sync` claim, no parallel executor), order-preserving removal is
  O(n) per component, two-component queries probe a sparse index per entry (no
  archetypes), and there are no query filters, change detection, events, or
  serialization. `iter2_mut::<A, A>` panics by design. None of the missing
  pieces are stubbed. Physics (impulses, friction, gravity) and 3D do not
  exist and are not partially implemented; audio exists as its own layer
  (ADR 0014, above) and collision resolution (below) is its own layer, not
  physics.
- **Collision scope:** axis-aligned boxes only — no rotation, circles, OBB/SAT,
  continuous collision, contact manifolds, friction, layers/masks, or impulse
  resolution. Overlaps are resolved by moving bodies out along the contact
  normal (half the depth each when both are movable); resting stacks are not a
  supported construct yet. Cells are emptied in place and reused across steps,
  but `pairs` sorts once per step — O(n log n) at gameplay scale, stated in
  `docs/collision.md`.
- **Scene scope:** the hierarchy is 2D-only, world-space, and rebuilt from
  scratch each step; there is no `LocalTransform`/`Transform` split, no 3D, no
  scene serialization or prefabs, no world-space-preserving reparent bake, and
  no multiple simultaneously active scenes (`SceneManager` runs one active scene
  per update). `GlobalTransform2d` read inside a running system sees the
  *previous* step's world (a documented one-step lag; readers after `update` see
  the current step). An entity that loses its `Transform2d` degrades to the
  parent frame rather than becoming invalid. The native graph is markedly slower
  than the Rust reference fold, as recorded above; the guide is `docs/scene.md`.
- **Input scope (ADR 0013):** devices are keyboard, mouse buttons, the cursor's
  absolute position, and focus — no gamepad (an analog pad is a future device
  kind), no IME/text input, and no key-label/layout mapping (the HID table maps
  physical codes only). Contexts are the only binding scope: there is no
  per-scene/per-moment auto-switching, no chord/sequence detection, and
  `value` is `held ? 1.0 : 0.0` plus this step's wheel deltas, not an analog
  signal. The native C backend measures slower than the Rust oracle in the
  recorded benchmark, as recorded above; the guide is `docs/input.md`.
- **Audio scope (ADR 0014):** fixed pools (256 sounds, 64 voices, a 256-command
  queue) with named errors at every limit, one output format (f32 stereo at the
  system's rate), and one decoded format family (the strict WAV subset above).
  There is no streaming playback (a sound is copied into the mixer whole, so
  the `2^24`-frame cap is also a memory consideration), no DSP effects, no
  reverb, no 3D spatialization, no recording, and no second mixer rate. The
  device path was exercised only to its `NO_DEVICE` outcome locally: no machine
  with a real sound card has run it yet, and no Valgrind run covers the module
  yet (exact command recorded above). `AudioSystem` is neither `Send` nor
  `Sync` by contract; audio runs on the control thread, and games that want
  otherwise own that decision explicitly. The guide is `docs/audio.md`.
- **Scripting scope:** one script per engine, single-threaded, no hot reload
  (scripts load at startup). The 64 MiB limit bounds memory, not time: a long
  `mikan_step` still costs the frame it runs in. Luau numbers cross the
  boundary as `f64` and are validated in `f32`, so the ECS determinism contract
  applies to gameplay, not to a fixed-point simulation. `--script` defaults to
  a repository-relative path, like `--assets`.
- **Capture:** blocking readback, PPM output only, restricted to RGBA8/BGRA8
  formats.
- **Interactive paths that are not automated:** window resize, occlusion,
  minimize/restore, close-button exit, and mouse-pointer movement (mouse
  *button presses* are automated only where input verification drives them).
  The code paths exist and are unit-tested where they can be, but they have
  not been exercised end to end.
- **Performance:** all numbers above describe a software rasterizer. No
  hardware-GPU measurement exists, so the 10,000-sprite figure is a measured
  floor of what the pipeline submits (one draw call), not a hardware estimate.

## Milestone changes (Phases 3–7 committed locally; Phases 8 and 9 committed and unpushed)

The facade milestone (`5ffe1c7`, `431d894`, documentation `92398f3`/`9c3e98b`)
plus the Phase 3 change set, committed locally on 2026-10-16 as eight commits
(not yet pushed): `991c17e` (core) → `a7c69f6` (assets) → `7c3c3a1` (render)
→ `2b9d66b` (mikan) → `201190d` (demo2d) → `162f304` (verification) → `2f37c02`
(docs) → `5c95578` (commit record).

In detail:

- **New crate:** `crates/assets` (`image.rs`, `sheet.rs`, tests) and its
  workspace wiring; `mikan` re-exports it.
- **`mikan-core`:** new `uv.rs` (`UvRect`, moved from `mikan-render`, with a
  collapsible `inset`); `camera.rs` gains `pixel_perfect`,
  `pixel_perfect_scale`, and `snap_to_physical_pixels`.
- **`mikan-render`:** `sprite.rs` gains rotation, `SamplerMode` (two bind
  groups per texture, sampler in the batch key), the 56-byte instance, and
  `FrameStats`; `texture.rs` gains `TextureEncoding`; `renderer.rs` gains
  `last_frame_stats`; `sprite.wgsl` rotates corners.
- **`mikan`:** re-exports of `mikan_assets` and the new types.
- **`demo2d`:** `atlas.rs` deleted; `assets.rs` (file loading + sRGB upload),
  `scene.rs` (three scenes, testable stress layout), extended `cli.rs`,
  reworked `demo.rs` and `main.rs`; committed `games/demo2d/assets/`.
- **Scripts:** `generate_assets.py`, `verify_showcase.{sh,py}`,
  `verify_stress.{sh,py}`, `verify_pixel_perfect.{sh,py}`, `verify_assets.sh`,
  `bench_sprites.sh`, `bench_report.py`; `lib.sh` and `verify.sh` extended.
- **CI:** the generator check and the four new Xvfb verification steps added
  to `.github/workflows/ci.yml`.
- **Docs:** ADR 0008, `docs/sprite-sheets.md`, and refreshed README,
  ARCHITECTURE, DEVELOPMENT, CHANGELOG, and this file.

The Phase 4 (ECS-lite) change set was committed as five commits
(`813cbc2`..`0f74e34`) on top of `5c95578`, listed here so the inventory is
complete:

- **New crate:** `crates/ecs` (`entity.rs`, `world.rs`, `schedule.rs`,
  `error.rs`, `tests/determinism.rs`, `examples/fixed_steps.rs`) and its
  workspace wiring; `mikan` depends on it and re-exports it.
- **`mikan`:** re-exports of `mikan_ecs`, `DeadEntity`, `Entity`, `Schedule`,
  and `World`.
- **`demo2d`:** `square.rs` deleted; new `player.rs` (components, resources,
  the four systems, pure interpolation, unit tests); `demo.rs` owns the
  `World`/`Schedule`/player entity and logs the ECS lines; `main.rs` module
  list.
- **Scripts:** new `verify_ecs.sh`; `lib.sh`, `verify.sh`, and the CI smoke
  job extended with the step.
- **Docs:** ADR 0009, `docs/ecs.md`, and refreshed README, ARCHITECTURE,
  DEVELOPMENT, CHANGELOG, and this file.

The Phase 5 (collision + Luau scripting) change set was committed as `5f63579`,
listed here so the inventory is complete:

- **New crates:** `crates/collision-sys` (`src/mikan_contacts.h`,
  `src/contacts.c`, `build.rs`, `src/lib.rs`, `tests/contacts.rs`),
  `crates/collision` (`broadphase.rs`, `reference.rs`, `tests/`,
  `examples/bench.rs`), `crates/script` (`src/lib.rs` with 18 tests),
  and their workspace wiring.
- **Root manifest:** workspace members and dependencies plus `mlua 0.11.6`
  (`luau` + `vendored`).
- **`mikan`:** re-exports of the collision and script crates with their common
  types.
- **`demo2d`:** new `blocks.rs` (the block layout, the `Collider` component,
  the `collide` system and its tests); `player.rs` gains `Controls` (replacing
  `MoveDirection`) and the five-system schedule; `demo.rs` loads the script and
  owns the per-step call and the log-once idle fallback; `cli.rs` gains
  `--script`; new `games/demo2d/scripts/player.luau`; `main.rs` module list.
- **Scripts:** new `verify_collision.sh`, `verify_script.sh`, and
  `bench_collision.sh`; `lib.sh`, `verify.sh`, and the CI smoke job extended
  with the collision and scripting steps.
- **Docs:** ADR 0010 and ADR 0011, `docs/collision.md`, `docs/scripting.md`,
  and refreshed README, ARCHITECTURE, DEVELOPMENT, FFI policy, ECS guide,
  CHANGELOG, and this file.

The Phase 7 (scene and entity system) change set was committed as `860d698`,
with a follow-up that moves the C kernel to the C driver as `07c6c33`, on top
of `5f63579`, together with the crate-directory renames
(`crates/mikan-*` → `crates/*`), listed here so the inventory is complete:

- **Crate renames:** every workspace crate directory drops the `mikan-` prefix
  (`crates/core`, `crates/assets`, `crates/ecs`, `crates/collision-sys`,
  `crates/collision`, `crates/scene-sys`, `crates/scene`, `crates/script`,
  `crates/platform`, `crates/render`, `crates/mikan`); package names are
  unchanged, so the renames do not touch the import graph.
- **New crates:** `crates/scene-sys` (`src/transform.c` and
  `src/mikan_transform.h`, `src/scene_graph.cpp` and
  `src/mikan_scene_graph.h`, `build.rs`, `src/lib.rs`, `src/graph.rs`,
  `tests/graph.rs`), `crates/scene` (`transform.rs`, `hierarchy.rs`,
  `despawn.rs`, `scene.rs`, `manager.rs`, `reference.rs`,
  `examples/bench_scene.rs`, `examples/hierarchy.rs`, `tests/cross_check.rs`,
  `tests/scene.rs`), and their workspace wiring.
- **`mikan-ecs`:** `Entity::to_bits`/`from_bits` (opaque `u64` identity), so the
  native graph and outside code can carry handles without ECS types.
- **`mikan`:** re-exports of the scene crate with its common types
  (`Transform2d`, `GlobalTransform2d`, `Hierarchy`, `Scene`, `SceneManager`, …).
- **`demo2d`:** new `renderable.rs` (the sprite collector over
  `(SpriteRenderer, GlobalTransform2d)`) and `hierarchy.rs` (the sun/planets/moon
  scene, the `Spin` system, and deferred despawn); `player.rs`, `blocks.rs`,
  and the classic `scene.rs`/`demo.rs` migrated to the transform hierarchy;
  `cli.rs` and `main.rs` gain the `hierarchy` scene and its controls and log
  lines.
- **Scripts:** new `verify_scene.{sh,py}` and `bench_scene.sh`; `lib.sh`,
  `verify.sh`, and the CI smoke job extended with the scene steps.
- **Docs:** ADR 0012, `docs/scene.md`, and refreshed README, ARCHITECTURE,
  DEVELOPMENT, FFI policy, CHANGELOG, and this file.

The Phase 8 input change set was committed as `773589e` (the bulk) and
`23be31e` (the follow-up fix, the build-resource tuning, and the
**documentation**), listed here so the inventory is complete:

- **New crates:** `crates/input-sys` (`src/mikan_input.h`, `src/input_backend.c`,
  `src/input_runtime.cpp`, `build.rs`, `src/lib.rs` with status mirrors and
  `keys.rs` HID constants, `src/backend.rs`/`src/runtime.rs` RAII,
  `src/reference.rs` the Rust normalizer oracle, `tests/backend.rs`,
  `tests/runtime.rs`, `tests/cross_check.rs`), `crates/input`
  (`src/lib.rs`, `src/map.rs`, `src/system.rs`,
  `examples/bench_input.rs`, `tests/integration.rs`), and their workspace
  wiring.
- **`mikan-platform`:** the per-frame `DeviceEvent` log (`MAX_FRAME_EVENTS`
  bounded drop-and-count, `take_events` drain, `dropped_events()`), mouse-wheel
  handling with `LineDelta` → pixel normalization, `end_frame` clearing log +
  wheel residual + edges, and `AppContext` lending `&mut InputState`
  (`input()` for the raw lane, `input_mut()` for the drain; the runner uses
  split-field borrows).
- **`mikan`:** re-exports of `mikan_input` with its common types
  (`InputSystem`, `ActionId`, `Binding`, `ContextId`, `WheelAxis`,
  `ActionState`, `InputError`) and of `DeviceEvent`.
- **`demo2d`:** new `actions.rs` (`ActionMapping`: `global` context,
  priority 10, Escape → quit; `gameplay` context, priority 0, WASD/arrows →
  movement, `F` → despawn the moon, wheel-vertical → scroll); `cli.rs` gains
  `--actions`; `demo.rs` splits `update` into the raw lane and the actions
  lane (drain → process → queries → `end_step`) over a shared `step_with`
  fixed-step helper, and the shutdown log gains the
  `actions: {fixed} fixed steps, {moved} moved, {wheel} wheel ticks` summary;
  `main.rs` module list.
- **Scripts:** new `bench_input.sh` and `verify_input_actions.sh`; `verify.sh`
  and the CI smoke job extended with the actions step.
- **Follow-up fix (`23be31e`):** `crates/input-sys/src/input_runtime.cpp`
  releases a held binding in **every** context that binds it, not only in the
  context arbitration would currently choose, so a release still reaches the
  context that consumed the press even after a higher-priority context was
  re-activated; `tests/runtime.rs` adds
  `release_reaches_the_context_that_consumed_the_press` (and nothing else in
  the runtime's semantics changed).
- **Build-resource tuning (`23be31e`):** `.cargo/config.toml` pins one Cargo
  job (cargo config files cannot carry `[profile]` tables), and the workspace
  `Cargo.toml` tunes the profiles itself — `[profile.dev]` keeps `debug = 1`
  and `[profile.release]` uses `lto = false` with 16 codegen units — so the
  default build stays within this machine's memory budget (`--jobs` restores
  parallelism).
- **Docs (`23be31e`, the remainder of this revision):** ADR 0013, `docs/input.md`, the native
  module inventory in `docs/ffi.md`, refreshed README, ARCHITECTURE,
  DEVELOPMENT, CHANGELOG, this file, and the CI step.

The Phase 9 audio change set was committed as `a3f77e8` (the feature, the
docs, the scripts, and the verification in one commit) followed by the commit
that recorded it here, listed so the inventory is complete:

- **New crates:** `crates/audio-sys` (`src/mikan_audio.h` — the ABI contract;
  `src/audio_backend.c` — the C device backend and the offline ring;
  `src/audio_runtime.cpp` — the C++ mixer runtime; `src/mikan_miniaudio_config.h`
  and `src/miniaudio_impl.c` — the vendored-miniaudio translation unit;
  `third_party/miniaudio/miniaudio.h` + `README.md` — the pinned vendored
  header, version 0.11.25 with its SHA-256 recorded; `build.rs`; `src/lib.rs`,
  `src/backend.rs`, `src/runtime.rs`, `src/system.rs` — the raw mirrors and
  RAII handles; `src/reference.rs` — the Rust mixer oracle; `tests/backend.rs`,
  `tests/runtime.rs`, `tests/raw.rs`, `tests/cross_check.rs`) and
  `crates/audio` (`src/lib.rs`, `src/sound.rs`, `src/voice.rs`,
  `src/system.rs`, `examples/bench_audio.rs`), with their workspace wiring and
  the root `[workspace.dependencies]` entries.
- **`mikan-assets`:** new `src/sound.rs` (`SoundData`, the strict WAV decoder,
  `SoundError`, `MAX_SOUND_FRAMES`) with its tests and re-exports.
- **`mikan`:** re-exports of `mikan_audio` with its common types and of
  `SoundData`/`SoundError` from `mikan-assets`.
- **`demo2d`:** `--audio` in `cli.rs` and the `AudioLane` in `demo.rs` (the
  device attempt with an explicit offline fallback, the blip on movement edges
  with pitch following the vertical direction, one pump plus a full drain per
  displayed frame, the shutdown summary), and the committed
  `games/demo2d/assets/blip.wav`.
- **Scripts:** new `verify_audio.sh` and `bench_audio.sh`;
  `scripts/generate_assets.py` writes and checks `blip.wav`; `verify.sh` and
  the CI smoke job gained the audio step.
- **Harness fix (the follow-up `fix` commit, which also records it here):** the
  platform runner logs `presenting frames` once its first frame outcome is
  `Presented`, and `start_demo` waits for that line before a script injects
  any key. winit queues every event that arrives while the renderer
  initializes, and a press plus release drained in one pump never reads as
  held — on this machine, memory pressure slowed llvmpipe's adapter
  enumeration to seconds, which is what exposed the race in
  `verify_input.sh` (the square could report zero movement while the `--audio`
  lane, tapped after a faster startup, passed). With the gate, input is only
  injected while the frame loop is live.
- **Docs:** ADR 0014, `docs/audio.md`, and refreshed README, ARCHITECTURE,
  DEVELOPMENT, CHANGELOG, FFI policy, and this file (including the pending
  Valgrind record and the no-sound-card limitation, both stated as pending
  rather than claimed).

## Next milestone

Ordered by intended value; each needs its own design note before implementation.

1. **Asset pipeline depth** — atlas packing and hot reload on top of
   `mikan-assets`, plus a streaming audio path (the decoded-sound cap is the
   first thing a long music track runs into; `docs/audio.md` documents the
   entry point).
2. **Audio depth, if the owner continues the line** — streaming voices, a
   per-voice effect graph (filter/gain nodes), and a listener model for
   spatialization. Each is a design note away; none is stubbed today.
3. **Platform and hardware verification** — run `verify_audio.sh` and the
   pending Valgrind command on a machine with a real sound card, runtime-verify
   Windows and macOS (and Wayland) per ADR 0005, and record a hardware-GPU run
   of the sprite benchmark next to the `llvmpipe` numbers.

Native C/C++ modules are no longer deferred in principle: ADR 0010 (the C
collision kernel), ADR 0011 (the vendored C++ Luau VM), ADR 0012 (the C
transform kernel and Mikan's own C++ scene graph), ADR 0013 (the C input
backend and Mikan's own C++ action runtime), and ADR 0014 (the C audio device
backend over vendored miniaudio and Mikan's own C++ mixer) introduced the first
five, each with its justification and honest measurements. Every further module
must still clear `docs/ffi.md` (ADR 0004) — or arrive through an equally
explicit ADR that states what changed, as ADR 0010 and ADR 0012 did.
