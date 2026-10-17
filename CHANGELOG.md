# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
The engine is pre-1.0: minor versions may change APIs, and those changes are
listed here.

## [Unreleased]

### Added

- `mikan-scene-sys` and `mikan-scene`: scenes and the entity transform
  hierarchy. `Transform2d` and `GlobalTransform2d` (rotation is the source of
  truth; the composed `(cos, sin)` pair is cached so the native kernel never
  computes trigonometry), a `Hierarchy` forest over entity handles (at most one
  parent, node `0` is the "no parent" sentinel, cycles rejected without mutating
  the graph, removal re-roots children, stable insertion and attachment order),
  a `DespawnQueue` flushed after the schedule, a `Scene` (one `World`, one
  `Schedule`, the ordered update: systems → `flush_despawns` →
  `propagate_transforms`), a `SceneManager` (generational `SceneId`s, exactly one
  active scene per update), and `reference`, an independent Rust compose kept as
  the test oracle. The hierarchy's topology is a hand-written C++ graph
  (`src/scene_graph.cpp`, one opaque handle, `extern "C"` entry points, status
  codes) and its composition a C kernel (`mikan_transform2d_compose` over a
  seven-`f32` `#[repr(C)]` struct); `mikan-scene-sys` is the second crate in the
  workspace allowed to use `unsafe` (override and reason recorded in its crate
  root), and `mikan-scene` and every consumer are safe Rust. The C kernel and the
  Rust reference are cross-checked bit for bit (through `to_bits`), including a
  large randomized hierarchy.
- `scripts/bench_scene.sh`: runs a 100,000-node hierarchy through the native
  graph and the Rust reference in release and records the result under
  `artifacts/bench/`. The recorded outcome is that **the native graph is
  markedly slower** (the reference gets topology for free); no speedup is
  claimed, and a Valgrind run of the benchmark binary reports zero errors and
  zero bytes lost (per `docs/ffi.md`).
- `mikan`: re-exports of `mikan_scene` with its common types (`Transform2d`,
  `GlobalTransform2d`, `Hierarchy`, `HierarchyError`, `Scene`, `SceneError`,
  `SceneId`, `SceneManager`, `DespawnQueue`, and the transform functions).
- `demo2d`: a fourth scene, `--scene hierarchy`: a sun (root), three planets
  (children of the sun), and a moon (child of the first planet) linked by
  `mikan-scene`, with `A`/`D` spinning the sun so the planets orbit rigidly and
  `F` despawning the moon through the deferred queue. The classic and hierarchy
  scenes draw through one `renderable::collect` walk over `(SpriteRenderer,
  GlobalTransform2d)`.
- Verification: `scripts/verify_scene.sh` — the startup layout (warm sun at
  `[480, 300]`, blue planet at `[640, 300]`, yellow star at `[720, 300]`), the
  moon's centroid staying one orbit radius (240 px) from the sun while its angle
  changes by more than 5° under `D`, and the star pixels disappearing while the
  sun and planets remain under `F`. Wired into `scripts/verify.sh` and the CI
  smoke job.
- Documentation: ADR 0012 (the scene module: the C kernel and hand-written C++
  graph, the architecture rationale, and the honest slower-than-Rust
  measurement, with ADR 0004 superseded for this module only), `docs/scene.md`,
  and refreshed README, ARCHITECTURE, DEVELOPMENT, FFI policy, STATUS, and this
  changelog.
- `mikan-collision-sys` and `mikan-collision`: 2D collision. A uniform-grid
  broad phase with deterministic (sorted, deduplicated) output; `Aabb`, `Pair`,
  and `Contact` with documented semantics (overlap means `depth > 0`, touching
  boxes are separated, the normal points first→second, non-finite coordinates
  yield separated results order-independently); one safe batch call `contacts`
  into the C99 narrow-phase kernel `mikan_contacts`; and `reference`, a Rust
  implementation of the same math kept as the test oracle and the benchmark
  baseline, cross-checked bit for bit (including a randomized 256-box case).
  `mikan-collision-sys` is one of the two crates in the workspace allowed to use
  `unsafe` (the ABI boundary), with the override and its reason recorded in the
  crate root; `mikan-collision` and every consumer are safe Rust.
- `scripts/bench_collision.sh`: runs the same 250,000 candidate pairs from 512
  boxes through the C kernel and the Rust reference in release and records the
  results under `artifacts/bench/`. The recorded outcome is parity within
  run-to-run noise (one run favors C, the next favors Rust); no speedup is
  claimed, and a Valgrind run of the benchmark binary reports zero errors and
  zero bytes lost.
- `mikan-script`: sandboxed Luau gameplay scripting. `ScriptEngine` loads a
  script (from file or source) and calls its global `mikan_step(dt, left,
  right, up, down, pos_x, pos_y)` once per fixed step, returning a direction
  and a speed; the engine validates the result (finite in `f32`, non-negative
  speed, directions longer than unit length normalized, deliberately shorter
  ones left alone). The VM is built from vendored Luau sources through `mlua`
  (`luau` + `vendored`; a C++ compiler is required to build), sandboxed with an
  explicit safe library set, Luau's own sandbox mode, and a 64 MiB memory
  limit. `ScriptError` (`Load`, `Compile`, `MissingFunction`, `Runtime`,
  `InvalidReturn`, `Vm`) carries the script name and, where Luau provides it,
  the line.
- `mikan`: re-exports of `mikan_collision` and `mikan_script` with their
  common types (`Aabb`, `Broadphase`, `Contact`, `Pair`, `ScriptEngine`,
  `ScriptError`, `StepInput`, `StepOutput`).
- `demo2d`: the classic scene gains three static collision blocks (one unit
  test guards that none of them intrudes on the y = 300 corridor the input and
  camera verifications walk), the player is a collider too, and a stateful
  `collide` system rebuilds the broad phase and resolves contacts once per
  fixed step — movable bodies (those with `Motion`) are pushed out half each
  when both are movable, anything without `Motion` stays put. The schedule is
  now `snapshot, input, movement, collide, clamp`, so collision runs after
  integration and before the world-bounds clamp. Movement decisions come from
  a Luau script: `--script PATH` (default `games/demo2d/scripts/player.luau`),
  loaded at startup with load errors fatal, and run-time errors logged once
  before the player idles for the step while the game keeps running.
- Verification: `scripts/verify_collision.sh` (the startup log reports the
  blocks and grid; a diagonal run stops against the first pillar's left face at
  x = 582 and slides to the floor clamp at y = 552; the y = 300 corridor stays
  unobstructed) and `scripts/verify_script.sh` (the default script reproduces
  the documented 3.5 px per moved step; a `(3, 4, 60)` script for exactly 60
  steps traces the normalized path to `[498, 324]`, distinguishable from the
  unnormalized `[570, 420]`; syntax errors, a missing `mikan_step`, and a
  missing file each fail startup with exit 1; a runtime error and a non-finite
  return are each logged exactly once and move nothing). Both wired into
  `scripts/verify.sh` and the CI smoke job.
- Documentation: ADR 0010 (2D collision, the C kernel, its honest parity
  measurement, and ADR 0004 superseded for this module only), ADR 0011 (Luau
  gameplay scripting — an owner-directed roadmap amendment recorded as its own
  decision, including the mlua pin, sandbox, and failure semantics),
  `docs/collision.md`, `docs/scripting.md`, and refreshed README, ARCHITECTURE,
  DEVELOPMENT, FFI policy, ECS guide, STATUS, and this changelog.
- `mikan-ecs`: a CPU-only crate (depends on `thiserror` only, nothing from the
  workspace) providing `Entity` (generational handles, `Display` as
  `{index}v{generation}`, LIFO slot reuse, stale handles never resolve),
  `World` (one insertion-ordered sparse-set storage per component type keyed
  by `TypeId`, typed resources with replace-and-return semantics,
  `iter`/`iter_mut`/`iter2`/`iter2_mut` queries, `component_count` diagnostics),
  `Schedule` (named systems run once each, in registration order, with the
  caller's `dt`; `names()` for logs and tests), and `DeadEntity` (the one
  error: a component operation on a dead handle). Removal preserves iteration
  order — an O(n) trade stated in the guide — and `iter2_mut` proves its two
  storages disjoint with `HashMap::get_disjoint_mut` instead of `unsafe`.
- `mikan`: re-exports of `mikan_ecs` and `DeadEntity`, `Entity`, `Schedule`,
  `World`.
- `demo2d`: the classic scene's player is an entity (`player.rs`): components
  `Position`, `PreviousPosition`, `Motion`, `Player`; resources `MoveDirection`
  and `WorldBounds`; systems `snapshot`, `input`, `movement`, `clamp` running
  once per fixed step in exactly that order, with the previous per-step
  arithmetic preserved bit for bit. Rendering interpolates through the pure
  `render_pose` and never mutates the simulation; shutdown logs the entity,
  fixed-step, and moved-step counts, and startup logs the system order.
- Verification: `scripts/verify_ecs.sh` — the startup log must list the player
  entity, its four components, and the system order; holding `D` must move the
  player exactly 3.5 px per *moved* fixed step (420 px/s at 120 Hz) and the
  shutdown counter must agree with the final position; a no-input run must keep
  the moved-step counter at zero. `scripts/verify.sh` and the CI smoke job run
  it.
- Documentation: ADR 0009 (ECS-lite decisions, determinism contract, and why
  the milestone adds no native module) and `docs/ecs.md` (the user guide:
  entities, components, resources, systems, determinism, performance limits);
  refreshed README, ARCHITECTURE, DEVELOPMENT, STATUS, and this changelog.
- `mikan-assets`: a CPU-only crate (`png`, `serde`, `serde_json`; no graphics
  dependency) providing `Image` (PNG decoded to RGBA8, every color type
  normalized, per-side and allocation limits, descriptive `ImageError`s) and
  `SpriteSheet`/`SheetFrame` (named frames from a documented Aseprite JSON
  subset, with `meta.image` resolved relative to the document, duplicate-name
  detection, and validation of sizes, rectangles, pivots, and unsupported
  rotated/trimmed frames). Frames convert to inset `UvRect`s so linear
  sampling cannot bleed between atlas frames; the format and the trade are
  documented in `docs/sprite-sheets.md`.
- `mikan-core`: `UvRect` (moved from `mikan-render`; `inset` now allows
  collapsing to a point, which is what a one-texel frame needs), and
  pixel-perfect camera support: `Camera2d::pixel_perfect`,
  `pixel_perfect_scale`, and `snap_to_physical_pixels`, so one virtual pixel
  maps to a whole number of physical pixels and the camera sits on the
  physical grid.
- `mikan-render`: per-sprite rotation (`Sprite::with_rotation`, baked as
  `(cos, sin)` into a 56-byte instance), per-sprite sampling
  (`SamplerMode::{Linear, Nearest}` as part of the batch key; every texture
  carries one bind group per mode, so filtering never costs a pipeline
  switch), `TextureEncoding::{Linear, Srgb}` on texture upload (PNGs keep
  their stored sRGB bytes and the GPU decodes on sample), and
  `Renderer::last_frame_stats()` returning `FrameStats { sprites, batches,
  draw_calls }` for the last submitted frame.
- `mikan`: re-exports of `mikan_assets` and the new core/render types
  (`Image`, `ImageError`, `SheetError`, `SheetFrame`, `SpriteSheet`,
  `UvRect`, `SamplerMode`, `TextureEncoding`, `FrameStats`).
- `demo2d`: three scenes — `classic`, `showcase` (rotation, tint, alpha,
  nearest vs linear, and PNG alpha at fixed verified positions), and `stress`
  (N sprites from one texture) — plus `--scene`, `--sampler`,
  `--pixel-perfect WxH`, `--assets DIR`, `--size WxH`, `--no-vsync`, and
  `--sprites N`; assets are committed PNGs loaded and validated at startup;
  `--stats` now also prints the last frame's sprites/batches/draw calls.
- Asset generator and verification: `scripts/generate_assets.py` (stdlib-only,
  deterministic, `--check` for CI), `scripts/verify_showcase.sh`,
  `scripts/verify_stress.sh`, `scripts/verify_pixel_perfect.sh`,
  `scripts/verify_assets.sh`, and an honest benchmark harness
  (`scripts/bench_sprites.sh` + `scripts/bench_report.py`); `scripts/verify.sh`
  and the CI smoke job run the new checks.
- Documentation: ADR 0008 (assets, samplers, rotation, pixel-perfect
  contract, benchmark honesty, and no native module in this milestone) and
  `docs/sprite-sheets.md`; refreshed README, ARCHITECTURE, DEVELOPMENT,
  STATUS, and this changelog.
- Workspace foundation: `mikan-core`, `mikan-platform`, `mikan-render`, and the
  `demo2d` application, with a pinned toolchain, shared lint configuration, and a
  committed lockfile.
- `mikan-core`: validated `EngineConfig`, clamped frame `Time`, `FixedTimestep`
  with a bounded step budget and interpolation alpha, `FrameOutcome`, `Color`,
  `Camera2d` (position, zoom, `world_to_screen`, pixel-space helper), and
  one-shot logging initialization.
- `mikan-platform`: the `App` lifecycle trait, `AppContext` (window handle, input,
  config, timing, exit requests), `InputState` with held/edge queries and
  focus-release handling, and a `winit`-based event loop runner with an
  occlusion/suspension aware wait policy.
- `mikan-render`: `wgpu` device and surface management with explicit handling of
  every swap chain state (`Success`, `Suboptimal`, `Timeout`, `Occluded`,
  `Outdated`, `Lost`, `Validation`), resize handling for zero-sized windows,
  device-lost reporting, an instanced sprite pipeline (flat colored or textured
  rectangles, atlas-ready UV regions, one draw call per consecutive texture run,
  painter order equals submission order), validated linear RGBA8 texture upload,
  a view/projection camera matrix, and optional single-frame RGBA8 readback.
- `mikan`: the engine facade an application depends on — a `Game` lifecycle
  trait that mirrors `App` hook for hook, an `Engine` handle (platform context
  plus renderer, with frame submission as the only mutable path),
  `mikan::run` with renderer creation, resize forwarding, and labeled
  renderer-startup failures, `Game::Error: From<RenderError>` error
  translation, and curated re-exports of the three layers.
- `demo2d`: a playable demo with fixed-timestep movement (WASD and arrow keys),
  delta-time-independent speed, Escape and window-close exit, `--camera`/`--zoom`
  options alongside `--frames`/`--capture`/`--stats`/`--timeout`, and a PPM
  writer for captured frames. Built on the `mikan` facade and depending on no
  other engine crate.
- Verification: `scripts/verify.sh` runs every gate plus the headless smoke
  test, `scripts/verify_input.sh` injects real X11 key events, and
  `scripts/verify_camera.sh` proves the follow camera keeps the moving player
  centered and that `--zoom` scales the rendered square.
- Documentation: architecture guide, development guide, FFI policy, ADRs
  (0006, 0007, and 0008 record why the sprites/camera, facade, and
  sprite-rendering milestones contain no native module), status report,
  contributing guide, and this changelog.
- CI: formatting, lint, test, and release build gates; a headless smoke test on
  `Xvfb` with a software Vulkan device that verifies a captured frame; and
  compile checks on Windows and macOS.

### Changed

- Crate directories dropped the `mikan-` prefix (`crates/mikan-core` →
  `crates/core`, and so on); package names are unchanged, so imports and the
  dependency graph are unaffected. Workspace members and path dependencies were
  updated accordingly.
- `demo2d`: the classic player's movement decisions now come from a Luau
  script through `mikan-script` instead of Rust code, and the `MoveDirection`
  resource became `Controls { direction, speed }` (the script returns a
  direction and a speed; the systems are unchanged downstream). With the
  default script the observable behavior — 420 px/s, unaccelerated diagonals —
  is exactly what the Rust implementation produced.
- Native code is no longer deferred in principle: `mikan-collision-sys`
  compiles C99 sources with warnings-as-errors, `mikan-scene-sys` compiles a C
  kernel and a hand-written C++ graph (warnings-as-errors, `-fno-exceptions
  -fno-rtti`), and `mikan-script` compiles the vendored C++ Luau VM, so a C and
  a C++ compiler are now build prerequisites. ADR 0004 remains the policy
  baseline; ADR 0010 and ADR 0012 supersede its measurement criterion for the
  collision and scene modules only, each reporting its result as measured
  (collision at parity, the scene graph slower than Rust).
- `demo2d`: the classic scene's player state and movement code moved from the
  `Square` struct into `mikan-ecs` components, resources, and systems; the
  fixed-step arithmetic and the rendered behavior are unchanged.
- `demo2d` no longer generates its atlas in code: it loads `atlas.png`,
  `atlas.json`, and `star.png` from `games/demo2d/assets` and uploads them as
  sRGB textures.
- `UvRect` now lives in `mikan-core` and is re-exported by `mikan-render`, so
  the asset layer can produce UVs without depending on the render layer.

### Removed

- `games/demo2d/src/square.rs` — the `Square` type, replaced by the player
  entity and its schedule in `games/demo2d/src/player.rs`.
- `games/demo2d/src/atlas.rs` — the procedural two-tile atlas, replaced by
  committed, generated PNG assets.

[Unreleased]: https://github.com/neverseeyouagainnnnn/mikan/commits/main
