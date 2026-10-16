# Mikan status

- **Date:** 2026-10-16
- **Revision:** `main` @ `0f74e34` ("docs: document the ECS-lite milestone").
  The Phase 3 change set is committed locally as eight commits
  (`991c17e`..`5c95578`), Phase 4 (ECS-lite) as five (`813cbc2`..`0f74e34`), and
  the Phase 5 (collision + Luau scripting) change set is **uncommitted in the
  working tree**. **Pushes and CI runs still require explicit authorization**,
  so the "Verification: CI" section below records the last pushed state, and
  every claim in this document comes from local runs on this machine.
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

- **`mikan-collision-sys`**, the one crate allowed to use `unsafe`: a C99
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

## What is implemented

| Area | What exists | Coverage |
| --- | --- | --- |
| `mikan-core` | `EngineConfig` + validation, `Color`, `Time` (clamped delta, zero first tick), `FixedTimestep` (bounded catch-up, interpolation alpha), `FrameOutcome`, `Camera2d` (position, zoom, `world_to_screen`, sanitizing `effective_zoom`, `pixel_space`, `pixel_perfect` integer zoom + snap, `pixel_perfect_scale`, `snap_to_physical_pixels`), `UvRect` (normalized region, collapsible half-texel `inset`), one-shot `init_logging` | 32 unit tests |
| `mikan-assets` | `Image` (PNG → RGBA8, all color types normalized, `MAX_IMAGE_DIMENSION` + checked allocation, `ImageError`), `SpriteSheet`/`SheetFrame` (Aseprite JSON subset, `meta.image` resolution, duplicate detection via a custom serde visitor, geometry/pivot validation, rotated/trimmed rejection, inset `uv`, `center_for_pivot`), `SheetError` | 40 unit tests, 1 doc test |
| `mikan-ecs` | `Entity` (generational handle, `Display` `{index}v{generation}`, LIFO slot reuse, stale-handle rejection), `World` (insertion-ordered sparse-set storage per component type, typed resources, `iter`/`iter_mut`/`iter2`/`iter2_mut`, `component_count`, `entity_count`, `Debug`), `Schedule` (named systems once each in registration order, `names`/`len`/`clear`), `DeadEntity` | 40 unit tests, 5 integration tests (determinism), 4 doc tests |
| `mikan-collision-sys` | the C99 narrow-phase kernel (`mikan_contacts` over plain `#[repr(C)]` data, warnings-as-errors `build.rs`), the raw binding with `# Safety` docs, and a safe batch wrapper with debug assertions on pair indices; the one crate allowed `unsafe` (override and reason recorded in its crate root) | 7 integration tests (kernel semantics, boundary and degenerate cases, index validation) |
| `mikan-collision` | `Aabb`/`Pair`/`Contact` with documented semantics, uniform-grid `Broadphase` (sorted, deduplicated, deterministic output), `contacts`, and `reference` — the Rust oracle and benchmark baseline, cross-checked bit for bit (including a randomized 256-box scene stepped 8 × 20,000 times) | 6 broad-phase integration tests, 3 cross-check integration tests, 2 doc tests |
| `mikan-script` | `ScriptEngine` (`from_file`/`from_source`, `call_step`, `name`, `used_memory`), `StepInput`/`StepOutput`, and `ScriptError` (`Load`, `Compile`, `MissingFunction`, `Runtime`, `InvalidReturn`, `Vm`) carrying the script name and line; explicit safe library set, Luau `sandbox(true)`, 64 MiB memory limit; vendored Luau through `mlua` 0.11.6 | 18 unit tests |
| `mikan-platform` | `App` lifecycle trait, `AppContext`, `InputState` (held state, press/release edges, cursor delta, focus release), `winit` runner with occlusion/suspension-aware waiting | 9 unit tests, 1 doc test |
| `mikan-render` | `wgpu` instance/adapter/device/queue/surface, format/alpha/present-mode selection, explicit handling of every swap chain state, resize (including zero-sized), device-lost reporting, instanced sprite pipeline (flat or textured, per-sprite rotation, tint, alpha, linear/nearest sampling as a batch key), texture upload with declared encoding (`Linear`/`Srgb`, validated), camera view/projection matrix (including pixel-perfect), `FrameStats { sprites, batches, draw_calls }`, RGBA8 capture readback | 27 unit tests, 1 doc test |
| `mikan` | the `Game` lifecycle over the platform `App` (renderer created before `init`, resizes forwarded before the game sees them, `shutdown` iff `init` ran), the `Engine` handle (`AppContext` + `&Renderer` + `render_frame`), labeled renderer-startup failures, curated re-exports of all seven engine crates (including `mikan_ecs`, `mikan_collision`, and `mikan_script` with their common types) | 3 unit tests, 2 doc tests |
| `games/demo2d` | built on the `mikan` facade: CLI (`--frames`, `--capture`, `--stats`, `--timeout`, `--scene`, `--sampler`, `--pixel-perfect`, `--assets`, `--size`, `--camera`, `--zoom`, `--no-vsync`, `--sprites`, `--script`, `--help`), three scenes, asset loading/upload/validation, the classic scene as a `mikan-ecs` world (player plus three static collision blocks, `collide` resolution, five ordered systems, interpolated rendering), a Luau gameplay script driving movement (load errors fatal, runtime errors logged once with an idle fallback), PPM writer, frame-time and draw statistics | 47 unit tests |

## Verification: build and tests (this revision, local)

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo check --workspace --all-targets` | pass |
| `cargo test --workspace` | **248 pass, 0 fail** — 216 unit tests (47 demo2d, 40 mikan-assets, 40 mikan-ecs, 32 mikan-core, 27 mikan-render, 18 mikan-script, 9 mikan-platform, 3 mikan), 21 integration tests (7 mikan-collision-sys contacts, 6 mikan-collision broad phase, 5 mikan-ecs determinism, 3 mikan-collision cross-check), and 11 doc tests (4 mikan-ecs, 2 mikan, 2 mikan-collision, 1 mikan-assets, 1 mikan-platform, 1 mikan-render) |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo build --workspace --release` | pass |
| `python3 scripts/generate_assets.py --check` | pass: atlas.png, star.png, atlas.json all match the generator |

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

## Verification: CI

**No CI run covers the Phase 3 and Phase 4 commits or the Phase 5 working tree:
Phase 3 and Phase 4 are committed locally but not pushed, and Phase 5 is not
even committed yet** (by policy, pushes require explicit authorization). The
last pushed state, `9c3e98b` (documentation for the facade milestone; runtime
code `92398f3`/`431d894`), is green in run
[38753924627](https://github.com/neverseeyouagainnnnn/mikan/actions/runs/38753924627):
fmt/check/tests/clippy/release, the Xvfb smoke job (smoke, input, camera), and
the Windows/macOS compile checks. The repository's `.github/workflows/ci.yml`
and `scripts/verify.sh` now also run the generator check, the ecs, collision,
and scripting verifications, and the showcase, stress, pixel-perfect, and
asset-failure verifications, so the next authorized push will exercise them in
CI. The Windows/macOS compile checks will then build the C kernel and the
vendored Luau C++ for the first time; the GitHub runners ship the required
toolchains, but that has not been exercised here (no push since they were
added).

## Verification levels by platform (ADR 0005)

| Platform | Level | Evidence |
| --- | --- | --- |
| Linux / X11 (software Vulkan) | **runtime-verified** | local smoke, input, camera, ecs, collision, scripting, showcase, stress, pixel-perfect, and asset-failure checks; CI smoke job for the previous milestone |
| Windows x86-64 | **compile-verified** for the previous milestone | CI `cargo check --workspace --all-targets`; the new crates — including the C collision kernel and the vendored Luau C++ — have not been compile-checked on Windows or macOS yet (no push since they were added) |
| macOS (arm64) | **compile-verified** for the previous milestone | as above |
| Linux / Wayland | not verified | path exists through `winit`; never exercised |
| Hardware GPU / Windows / macOS runtime | not verified | no such machine in the verification environment |

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
  pieces are stubbed. Physics (impulses, friction, gravity), audio, and 3D do
  not exist and are not partially implemented; collision resolution (below) is
  its own layer, not physics.
- **Collision scope:** axis-aligned boxes only — no rotation, circles, OBB/SAT,
  continuous collision, contact manifolds, friction, layers/masks, or impulse
  resolution. Overlaps are resolved by moving bodies out along the contact
  normal (half the depth each when both are movable); resting stacks are not a
  supported construct yet. Cells are emptied in place and reused across steps,
  but `pairs` sorts once per step — O(n log n) at gameplay scale, stated in
  `docs/collision.md`.
- **Scripting scope:** one script per engine, single-threaded, no hot reload
  (scripts load at startup). The 64 MiB limit bounds memory, not time: a long
  `mikan_step` still costs the frame it runs in. Luau numbers cross the
  boundary as `f64` and are validated in `f32`, so the ECS determinism contract
  applies to gameplay, not to a fixed-point simulation. `--script` defaults to
  a repository-relative path, like `--assets`.
- **Capture:** blocking readback, PPM output only, restricted to RGBA8/BGRA8
  formats.
- **Interactive paths that are not automated:** window resize, occlusion,
  minimize/restore, close-button exit, mouse input, and focus-loss release. The
  code paths exist and are unit-tested where they can be, but they have not
  been exercised end to end.
- **Performance:** all numbers above describe a software rasterizer. No
  hardware-GPU measurement exists, so the 10,000-sprite figure is a measured
  floor of what the pipeline submits (one draw call), not a hardware estimate.

## Milestone changes (Phase 3 and Phase 4 committed to `main`; Phase 5 uncommitted)

The facade milestone (`5ffe1c7`, `431d894`, documentation `92398f3`/`9c3e98b`)
plus the Phase 3 change set, committed locally on 2026-10-16 as eight commits
(not yet pushed): `991c17e` (core) → `a7c69f6` (assets) → `7c3c3a1` (render)
→ `2b9d66b` (mikan) → `201190d` (demo2d) → `162f304` (verification) → `2f37c02`
(docs) → `5c95578` (commit record).

In detail:

- **New crate:** `crates/mikan-assets` (`image.rs`, `sheet.rs`, tests) and its
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

- **New crate:** `crates/mikan-ecs` (`entity.rs`, `world.rs`, `schedule.rs`,
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

The Phase 5 (collision + Luau scripting) change set is **uncommitted in the
working tree** on top of `0f74e34`, listed here so the inventory is complete:

- **New crates:** `crates/mikan-collision-sys` (`src/mikan_contacts.h`,
  `src/contacts.c`, `build.rs`, `src/lib.rs`, `tests/contacts.rs`),
  `crates/mikan-collision` (`broadphase.rs`, `reference.rs`, `tests/`,
  `examples/bench.rs`), `crates/mikan-script` (`src/lib.rs` with 18 tests),
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

## Next milestone

Ordered by intended value; each needs its own design note before implementation.

1. **Asset pipeline depth** — atlas packing and hot reload on top of
   `mikan-assets`.
2. **Platform and hardware verification** — runtime-verify Windows and macOS
   (and Wayland) per ADR 0005, and record a hardware-GPU run of the sprite
   benchmark next to the `llvmpipe` numbers.

Native C/C++ modules are no longer deferred in principle: ADR 0010 (the C
collision kernel) and ADR 0011 (the vendored C++ Luau VM) introduced the first
two, each with its justification and honest measurements. Every further module
must still clear `docs/ffi.md` (ADR 0004) — or arrive through an equally
explicit ADR that states what changed, as ADR 0010 did.
