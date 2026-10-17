# Mikan architecture

This document describes the architecture that exists in this repository today.
Planned work is confined to the "Extension points" section and is clearly marked
as not implemented.

## 1. Goals and non-goals

### Goals

1. **Correctness and stability first.** Recoverable graphics conditions are
   handled deliberately; unrecoverable ones are explicit failures that reach the
   user as an error and a non-zero exit code.
2. **A clear, maintainable architecture.** Small crates with single
   responsibilities and a one-way dependency graph.
3. **Measurable performance.** No performance claim is made without a
   reproducible measurement (see the benchmark protocol in `DEVELOPMENT.md`).
4. **Cross-platform architecture.** Windows, Linux, and macOS are supported
   architecturally; only Linux has been runtime-verified so far (ADR 0005).
5. **Modularity and explicit ownership.** Every GPU and OS resource has exactly
   one owner, and ownership is expressed by the type system.
6. **A practical development experience.** One command builds, one runs, one
   tests the workspace.
7. **Learning value without sacrificing discipline.** The code is meant to be
   read; no unexplained magic, no speculative abstraction.

### Non-goals (for now)

- A plugin system, reflection, or a dependency-injection container.
- A render graph, material system, or deferred renderer.
- An archetype ECS, job system, or custom allocator. (A deliberately small
  ECS-lite crate exists; see §2.)
- 3D rendering, PBR, shadows, or post-processing.
- Native C/C++ modules without a concrete, documented justification (ADR 0004).
  The native modules are recorded in ADR 0010 (collision), ADR 0011 (Luau), and
  ADR 0012 (scene), and ADR 0013 (input); none is speculative.

## 2. Crate responsibilities

```
crates/core            no dependencies on windowing or graphics
crates/assets          PNG decoding and sprite-sheet metadata; no graphics
crates/ecs             entity/component storage and system scheduling; no workspace deps
crates/collision-sys   the C99 narrow-phase kernel and its ABI mirror; uses unsafe
crates/collision       safe collision face: broad phase, reference math, batch contacts
crates/scene-sys       the C compose kernel and the C++ scene graph; uses unsafe
crates/scene           safe scene face: transforms, hierarchy, scenes, manager
crates/input-sys       the C device backend and the C++ action runtime; uses unsafe
crates/input           safe input face: InputSystem, actions, contexts, key mapping
crates/script          sandboxed Luau VM and the gameplay step contract
crates/platform        winit is its only OS dependency; no graphics
crates/render          wgpu is its only graphics dependency; no event handling
crates/mikan                 facade: one lifecycle that composes the engine crates
games/demo2d                 application: implements the facade's Game trait, owns gameplay
```

### `mikan-core`

Platform-independent vocabulary and timing. It has no dependency on `winit`,
`wgpu`, or any windowing/graphics crate, so every other layer can depend on it
without pulling in the platform stack.

- `EngineConfig` + `ConfigError` — window size, title, vsync, fixed timestep
  rate, step budget, maximum frame delta, clear color; validated before startup.
- `Time` — monotonic frame timing with a configurable delta clamp.
- `FixedTimestep` — accumulator that converts frame deltas into a bounded number
  of fixed steps plus an interpolation alpha.
- `FrameOutcome` — the shared result of a frame: `Presented`, `Skipped`, `Idle`.
- `Camera2d` — position plus zoom, with `world_to_screen` as the reference
  transformation and a sanitizing `effective_zoom`; `pixel_perfect` derives an
  integer zoom and snaps the camera to the physical pixel grid for pixel art.
  No graphics dependency, so gameplay math and rendering share one definition.
- `UvRect` — a normalized texture region with the half-texel `inset` atlas
  sampling needs; shared vocabulary rather than a render-only type.
- `Color` — linear RGBA, deliberately independent of `wgpu::Color`.
- `init_logging` — one-shot `env_logger` initialization honoring `RUST_LOG`.

### `mikan-assets`

CPU-side asset loading: files in, pixels and metadata out. It depends on
`mikan-core` only — no window, no device, no `wgpu` — so decoding is testable
(and usable) without a GPU.

- `Image` — a PNG decoded to the engine's canonical representation (RGBA8, top
  row first, straight alpha, stored bytes kept), with per-side dimension and
  allocation limits and descriptive `ImageError`s. The `png` crate does the
  bit-level decoding; Mikan owns normalization, limits, and errors.
- `SpriteSheet` / `SheetFrame` — a documented subset of Aseprite's JSON export:
  named pixel rectangles plus optional duration and pivot, validated at load
  (duplicates, zero sizes, out-of-bounds rectangles, pivots, rotated/trimmed
  frames). `SheetFrame::uv` converts a frame to an inset `UvRect` so linear
  sampling never reads a neighbouring frame's texels. The format is documented
  in `docs/sprite-sheets.md`.

The split is deliberate: the render layer accepts pixels (`TextureDescriptor`)
and knows nothing about files; the application connects the two, and chooses
the texture encoding (`Linear` for procedural data, `Srgb` for standard PNGs).

### `mikan-ecs`

Entity storage and systems, the CPU-side model a game simulates. It has no
workspace dependency at all (only `thiserror`), so gameplay data and schedules
are testable without a window, a device, or the engine runtime.

- `Entity` — a generational handle (`index` + `generation`, `Display` as
  `{index}v{generation}`). Slots are recycled in LIFO order and generations
  increment, so a despawned handle never resolves again and stale handles are
  errors, not aliases.
- `World` — entity allocation, one sparse-set storage per component type keyed
  by `TypeId`, and typed resources for data no entity owns. Iteration is
  insertion-ordered and removal preserves that order (an O(n) trade stated in
  ADR 0009). `iter`/`iter_mut` query one component, `iter2`/`iter2_mut` two.
- `Schedule` — named systems run once each, in registration order, with the
  `dt` the caller passes. No dependency graph, no parallelism, no timer of its
  own: `mikan-core`'s fixed timestep remains the only scheduler. The user guide
  is `docs/ecs.md`.
- `DeadEntity` — the single error: a component operation targeted an entity
  that is not alive.

The crate is deliberately *lite*: no archetypes, query caching, change
detection, or events. It is single-threaded by design and makes no `Send`/
`Sync` claim. ADR 0009 records the decisions and why this milestone adds no
native module.

### `mikan-collision-sys` and `mikan-collision`

The collision boundary, split the way `docs/ffi.md` requires. `-sys` owns the
C99 kernel (`mikan_contacts`, one batch function over plain `#[repr(C)]`
structs), its warnings-as-errors `build.rs`, and the raw `extern "C"`
declaration; it is one of the crates where `unsafe` is allowed, with the reason
recorded in its crate root. `mikan-collision` is the safe face: `Aabb`/`Pair`/
`Contact`, the uniform-grid `Broadphase`, the batch `contacts` call, and
`reference` — a Rust implementation of the same math kept as the test oracle
and the benchmark baseline (the two are cross-checked bit for bit). Resolution
policy is not decided here; the demo's `collide` system applies contacts. The
guide is `docs/collision.md`, the decision ADR 0010 — including the honest
parity-with-Rust benchmark result and why the kernel exists anyway.

Neither crate depends on the rest of the engine, so collision math is testable
without a window or a GPU.

### `mikan-scene-sys` and `mikan-scene`

The scene boundary, split the same way. `-sys` owns a C compose kernel
(`mikan_transform2d_compose`, one batch-shaped function over a seven-`f32`
`#[repr(C)]` struct) and a hand-written C++ scene graph (one opaque
`mikan_scene_graph` handle with `extern "C"` entry points and status codes); its
`build.rs` compiles both with warnings-as-errors and `-fno-exceptions
-fno-rtti`, and `unsafe` is allowed there with the reason recorded in the crate
root. `mikan-scene` is the safe face: `Transform2d`/`GlobalTransform2d`, the
`Hierarchy` RAII wrapper, the `DespawnQueue`, `Scene` (one world, one schedule,
and the update order), `SceneManager`, `propagate_transforms`, and `reference` —
an independent Rust compose kept as the test oracle and benchmark baseline (the
two are cross-checked bit for bit). The guide is `docs/scene.md`, the decision
ADR 0012 — including the honest result that the native graph is slower than a
Rust array fold, and why it exists anyway. Neither crate depends on the rest of
the engine, so scene math and topology are testable without a window or a GPU.

### `mikan-input-sys` and `mikan-input`

The input boundary, split the same way. `-sys` owns a C device backend
(`mikan_input_backend`: the canonical event model, raw device state, and
normalization — duplicate press/release suppression, OS-repeat handling, move
and wheel-delta coalescing, synthetic releases on focus loss) and a
hand-written C++ action runtime (`mikan_input_runtime`: bindings grouped into
prioritized contexts, per-event arbitration, and the per-action state machine
with `just_pressed`/`just_released`/`repeating`/`value` semantics). Its
`build.rs` compiles each file with warnings-as-errors and C++ with
`-fno-exceptions -fno-rtti`; `unsafe` is allowed there with the reason recorded
in the crate root, and `reference` is an independent Rust normalizer kept as
the test oracle (cross-checked bit for bit against the C backend). The
two native layers never call each other: Rust drains the backend and feeds the
runtime in one batch per fixed step. `mikan-input` is the safe face:
`InputSystem` (owns both native halves, translates `DeviceEvent`s, maps
statuses to `InputError`s), `ActionId`/`ContextId`, `Binding`/`WheelAxis`, and
the winit → HID code map. The guide is `docs/input.md`, the decision ADR 0013.

### `mikan-script`

Sandboxed gameplay scripting. `ScriptEngine` loads a Luau script and calls its
global `mikan_step` once per fixed step: arguments in, three numbers out,
validated by the engine (finite `f32`s, non-negative speed, over-long
directions normalized). The VM is built from vendored Luau sources through
`mlua` (`luau` + `vendored`), so a C++ compiler is required to build, and it is
sandboxed with an explicit safe library set, Luau's sandbox mode, and a 64 MiB
memory limit. Errors carry the script name and line; load-time failures are
fatal, run-time failures are logged once and recovered. The script owns intent
only — it cannot see the ECS, the renderer, or the clock. The guide is
`docs/scripting.md`; the decision ADR 0011.

### `mikan-platform`

Everything that touches the operating system's event and input model.

- `App` — the lifecycle trait: `init`, `resize`, `update`, `render`, `shutdown`.
- `AppContext` — access to the window handle, the input state (`&` for the raw
  lane, `&mut` for the event log a step may drain), config, frame time, and
  interpolation alpha, plus `request_exit`.
- `InputState` — keyboard and mouse held/edge queries, cursor delta, and focus
  handling that releases keys when focus is lost, plus the per-frame
  `DeviceEvent` log the input system drains (`take_events`), wheel-tick
  normalization to pixels, a bounded drop-and-count mode, and `end_frame`
  clearing all of it.
- `run` — creates the window and drives `App` from a `winit` event loop; the
  only place that knows about `WindowEvent`.

It deliberately does **not** depend on `mikan-render`, which keeps graphics out
of event handling.

### `mikan-render`

Everything between a window handle and a presented frame.

- `Renderer` — `wgpu` instance, adapter, device, queue, and surface, plus
  reconfiguration and recovery.
- `RenderWindow` — a blanket-implemented trait over `raw-window-handle`
  providers, so the renderer needs no windowing library dependency. `winit`
  windows, SDL windows, and anything else implementing `raw-window-handle`
  satisfy it.
- `Sprite` / `UvRect` / `Frame` — a rectangle in world space, flat or textured
  through an atlas-ready UV rectangle, with optional rotation and a per-sprite
  sampler mode, and the frame description: clear color, optional camera, and
  the sprites to draw. `FrameStats` reports what the last frame submitted
  (sprites, batches, draw calls).
- `Texture` / `TextureDescriptor` / `TextureEncoding` — textures uploaded from
  tightly packed RGBA8 pixels and validated before any GPU resource is created;
  the encoding (`Linear` default, `Srgb` for standard PNG bytes) is declared at
  upload, and the GPU decodes on sample. A texture is usable only with the
  `Renderer` that created it.
- `CaptureBuffer` — optional single-frame readback (RGBA8) used by tests and
  tooling.

### `mikan`

The composition layer an application depends on. It contains no windowing or
graphics code of its own; it owns the order in which the other two layers are
created, driven, and torn down.

- `Game` — the application lifecycle, mirroring `App` hook for hook (`init`,
  `resize`, `update`, `render`, `shutdown`) with an associated error type
  bounded by `From<RenderError>`.
- `Engine` — the handle callbacks receive: `ctx()` exposes the `AppContext`,
  `renderer()` returns `&Renderer` for queries and texture creation, and
  `render_frame` is the only path that touches `&mut Renderer`.
- `run` — creates the renderer before `Game::init`, forwards every resize to
  the renderer before the game sees it, delivers `shutdown` if and only if
  `init` ran, and translates game errors into `PlatformError`. The event loop
  itself remains `mikan_platform::run`; the facade adds one adapter, not a
  second runtime.
- Curated re-exports of the engine crates, including `mikan_assets`,
  `mikan_ecs`, `mikan_collision`, `mikan_scene`, `mikan_script`, and
  `mikan_input`, so an application needs one dependency for the common case.

### `games/demo2d`

The application. It implements the facade's `Game` trait, owns gameplay state
(the classic scene's player entity and the `mikan-ecs` schedule that drives it,
in `player.rs`; the sun/planet/moon hierarchy in `hierarchy.rs`), and contains
the command-line interface, four scenes (classic, hierarchy, showcase, stress),
asset loading and upload, the PPM writer, and the frame-time and draw
statistics. The classic and hierarchy scenes are built on `mikan-scene`: their
sprites are collected from `(SpriteRenderer, GlobalTransform2d)` entities by
`renderable.rs`, and the hierarchy's rigid rotation and deferred despawn are the
scene system's observable behavior. Its assets (`atlas.png`, `atlas.json`,
`star.png`) are committed files produced by `scripts/generate_assets.py` and
loaded through `mikan-assets` at startup. It depends on `mikan` only; the
other layers reach it through the facade's re-exports.

## 3. Dependency direction

```
                  +---------------+        +--------------+
                  |  mikan-core   |        |  mikan-ecs   |
                  +-------+-------+        |  (thiserror; |
                          |                |  no workspace|
      +-------------------+------------+   |  deps)       |
      |                   |            |   +------+-------+
+-----+-----+   +---------+-----+  +---+-------+  |
| mikan-    |   | mikan-assets  |  | mikan-    |  |
| platform  |   | (png, serde)  |  | render    |  |
+-----+-----+   +---------+-----+  +---+-------+  |
      |                   |            |          |
      |     +-------------+------------+----------+-------------------+
      |     |                                                       |
      |     |  +------------------------+   +--------------------+  |
      |     |  | mikan-collision-sys    |   | mikan-script       |  |
      |     |  | (C99 kernel; unsafe)   |   | (mlua + vendored   |  |
      |     |  +-----------+------------+   |  Luau, C++)        |  |
      |     |              |                +---------+----------+  |
      |     |  +-----------+------------+             |             |
      |     |  | mikan-collision (safe) |             |             |
      |     |  +-----------+------------+             |             |
      |     |  +------------------------+             |             |
      |     |  | mikan-scene-sys        |             |             |
      |     |  | (C kernel + C++ graph; |             |             |
      |     |  |  unsafe)               |             |             |
      |     |  +-----------+------------+             |             |
      |     |  +-----------+------------+             |             |
      |     |  | mikan-scene (safe)     |             |             |
      |     |  +------------------------+             |             |
      |     |  +------------------------+             |             |
      |     |  | mikan-input-sys        |             |             |
      |     |  | (C backend + C++       |             |             |
      |     |  |  runtime; unsafe)      |             |             |
      |     |  +-----------+------------+             |             |
      |     |  +-----------+------------+             |             |
      |     |  | mikan-input (safe)     |             |             |
      |     |  +------------------------+             |             |
      +-----+------------------------+----------------+-------------+
                                   |
                              +----+----+
                              |  mikan  |   (facade: Game, Engine, run)
                              +----+----+
                                   |
                              +----+----+
                              | demo2d  |   (depends only on mikan)
                              +---------+
```

Rules that keep this acyclic and comprehensible:

- `mikan-core`, `mikan-ecs`, `mikan-collision-sys`, `mikan-scene-sys`,
  `mikan-input-sys`, and `mikan-script` depend on nothing else in the workspace
  (third-party crates aside, `thiserror`, `winit` for input-sys's HID constants,
  and `mlua` respectively): the ECS is simulation data and scheduling, the
  collision and transform kernels are plain data in and out, the scene graph
  holds topology only, the input backend/runtime pair knows nothing about
  windows or rendering, and the script VM knows nothing about the engine.
- `mikan-collision` depends on `mikan-collision-sys` only, `mikan-scene`
  on `mikan-scene-sys` only, and `mikan-input` on `mikan-input-sys` plus
  `mikan-platform` (to translate its `DeviceEvent`) — the safe wrapper pattern
  `docs/ffi.md` prescribes.
- `mikan-assets` depends on `mikan-core` only: no windowing, no graphics.
- `mikan-platform` and `mikan-render` never depend on each other, and
  `mikan-render` does not depend on `mikan-assets` — the application glues
  decoded pixels into `TextureDescriptor`.
- `mikan` may depend on all engine crates; it re-exports them so
  applications need one dependency. Applications that need custom wiring can
  still use `mikan_platform::App` directly.
- No crate depends on an application.

The renderer's window requirement is expressed as the `RenderWindow` trait rather
than a dependency on `mikan-platform`, which is why the two layers can stay
siblings. The facade passes the window handle from `AppContext` to
`Renderer::new`; an application that wires the layers by hand passes it itself.

## 4. Application lifecycle

```
run(config, app)                       mikan-platform::run
  |
  +-- config.validate()                reject invalid configuration early
  +-- EventLoop::new()                 winit
  +-- event_loop.run_app(runner)
        |
        +-- resumed()                  first time: create the window,
        |                              then App::init
        |
        +-- window_event(...)
        |     Resized(size)         -> App::resize
        |     KeyboardInput/...     -> InputState
        |     Focused(false)        -> InputState::release_all
        |     Occluded(bool)        -> pause/resume frame requests
        |     RedrawRequested       -> the frame (see 5)
        |     CloseRequested        -> App::shutdown, exit
        |
        +-- about_to_wait()            poll while drawing, wait while idle
        |
        +-- suspended()                stop requesting frames
        +-- exiting()                  App::shutdown (exactly once)
  |
  +-- return the first fatal error, if any
```

Errors from `App::init`, `App::update`, and `App::render` are recorded, logged at
`error`, and cause the event loop to exit and `run` to return `Err`. `main`
prints the error and exits with a failure code. `App::shutdown` always runs
exactly once, whether the exit was orderly or caused by an error.

When an application uses the facade, `mikan::run` wraps the game in an adapter
that implements `App` and delegates to `mikan_platform::run`; the flow above is
unchanged. The adapter creates the renderer immediately before `Game::init`,
forwards `resize` to the renderer before calling the game's hook, and delivers
`Game::shutdown` if and only if `Game::init` was invoked — including when it
returned an error.

## 5. Game loop responsibilities

The loop is driven by `RedrawRequested`, never by a busy-wait. One frame is:

```
tick():                    Time::tick() -> clamped delta (max_frame_delta)
schedule:                  FixedTimestep::advance(delta) -> steps (<= max_steps_per_frame)
for each step:             App::update(ctx, fixed_dt)
once:                      App::render(ctx) -> FrameOutcome
if steps > 0:              InputState::end_frame()  (consume input edges)
continue?                  Presented/Skipped -> request another redraw
                           Idle             -> ControlFlow::Wait until events arrive
```

Properties this gives us:

- **Simulation is deterministic in time.** Gameplay sees a constant `dt`.
- **Update logic has one visible order.** An application runs its `mikan-ecs`
  schedule once per fixed update; systems execute once each, in registration
  order, and the schedule has no timer of its own — the platform decides when
  it runs at all.
- **Input is never dropped.** Press and release edges are consumed only by a
  frame that ran at least one fixed update, so a tap that lands between two
  simulation steps is observed by the next one. (With rendering at 600 fps and a
  120 Hz simulation, most frames run zero steps, so this is the common case, not
  an edge case.)
- **Presentation is decoupled.** Rendering happens once per frame and may
  interpolate with `ctx.alpha()` between the previous and current simulation
  states, so a 60 Hz display and a 120 Hz simulation still look smooth.
- **A long frame cannot cascade.** `Time` clamps the measured delta, and
  `FixedTimestep` drops the backlog once `max_steps_per_frame` is reached and
  counts the event (`clamp_count`). Simulation never tries to replay a stall.
- **An invisible window costs nothing.** `FrameOutcome::Idle`, occlusion, and
  suspension all switch the loop to `ControlFlow::Wait`, so there is no
  unbounded spin while nothing can be drawn.

## 6. Rendering lifecycle

`Renderer::new(window, size, settings)`:

1. Create the `wgpu::Instance`, tied to the window's display handle (required by
   the GLES backend on Wayland; ignored by Vulkan/Metal/DX12).
2. Create the surface from the window handle. The renderer keeps an `Arc` to the
   window, so the surface cannot outlive its window.
3. Request an adapter compatible with that surface; log name, backend, and device
   type, and warn when running on a CPU (software) device.
4. Request a device and queue with `Features::empty()` and `Limits::default()`
   (WebGPU defaults), and install a device-lost callback that records the reason.
5. Choose a surface configuration: prefer an sRGB format if offered, add
   `COPY_SRC` usage when the surface supports it (frames become capturable), and
   select `AutoVsync` or `AutoNoVsync`.
6. Build the sprite pipeline (including the 1×1 fallback texture) and configure
   the surface (skipped while the window has zero area).

Per frame, `render_frame` classifies the swap chain state and acts:

| State | Action | Caller sees |
| --- | --- | --- |
| `Success` | draw, submit, present | `Presented` |
| `Suboptimal` | draw, present, reconfigure next frame | `Presented` |
| `Timeout` | skip the frame | `Skipped` |
| `Occluded` | draw nothing | `Idle` |
| `Outdated` | reconfigure, skip this frame | `Skipped` |
| `Lost` | recreate the surface, reconfigure, skip this frame | `Skipped` |
| `Validation` | fail | `Err(SurfaceValidation)` |

At most one recovery action is taken per call, so recovery cannot loop inside a
single frame. Device loss is reported separately: the callback records it and the
next `render_frame` returns `Err(DeviceLost)`, which stops the application with a
clear message instead of producing corrupt frames.

`Renderer::resize` records the new size and reconfigures. A zero-sized window
(minimized) is recorded but not configured, because swap chains must be non-zero;
frames report `Idle` until a usable size returns.

### Sprite rendering

The geometry primitive is `Sprite`: center, size, color, rotation, an optional
texture with a UV rectangle, and a sampler mode, all in world units. A frame
renders through an optional `Camera2d`; without one, `Camera2d::pixel_space`
maps world units 1:1 to physical pixels with the origin at the top-left
corner. `Camera2d::pixel_perfect` instead derives an integer zoom from a
virtual screen size and snaps the camera so whole world coordinates land on
whole physical pixels. Every frame is drawn with:

- one static six-vertex unit square shared by all sprites,
- one instance per sprite (56 bytes: `center`, `size`, rotation as
  `(cos, sin)`, `color`, and the UV rectangle),
- one instanced draw call per **consecutive run of sprites that share a
  texture and a sampler mode** — painter order equals submission order,
  sprites are never reordered, and solid sprites share a 1×1 white fallback
  texture,
- one camera uniform (a `mat4x4` view/projection matrix built from
  `Camera2d`), and, per texture, one bind group per `SamplerMode` (linear and
  nearest), so filtering is a batching decision rather than a pipeline switch.

`Renderer::last_frame_stats()` reports `FrameStats { sprites, batches,
draw_calls }` for the last submitted frame, which is how the verification
scripts check that 10,000 sprites really batch into one draw call instead of
asserting it in prose.

The instance buffer grows geometrically when a frame needs more instances than
it currently holds. Texture descriptors are validated (non-zero, within
`max_texture_dimension_2d`, exact data length) before any GPU resource is
created. Textures are tightly packed RGBA8 with a single mip level, uploaded
from memory, and carry a declared `TextureEncoding`: `Linear` (the default,
exact backward compatibility with procedural data) or `Srgb` for standard PNG
bytes, which the GPU decodes on sample. Decoding files is `mikan-assets`'
job; the renderer never reads a path.

## 7. Error-handling strategy

| Layer | Error type | Policy |
| --- | --- | --- |
| `mikan-core` | `ConfigError` | Validation at startup; no runtime panics for bad input |
| `mikan-assets` | `ImageError`, `SheetError` | Files are validated at load: limits, encodings, rectangles, pivots; a bad asset is an error before it reaches the GPU |
| `mikan-ecs` | `DeadEntity` | A component operation on a dead handle returns it; the only panic is the documented `iter2_mut` misuse (two identical component types) |
| `mikan-collision-sys` / `mikan-collision` | none | The kernel has no error channel: plain data in, plain data out, with debug assertions on the one precondition (pair indices) and separated results as the defined answer to degenerate input |
| `mikan-scene-sys` | `GraphStatus` (`OK`, `UNKNOWN_NODE`, `CYCLE`, `BAD_ARGUMENT`) | No panics across the boundary: every entry point validates and returns a status code, and invalid arguments (including null handles) are reported, not crashed |
| `mikan-scene` | `HierarchyError` (`UnknownEntity`, `AlreadyPresent`, `Cycle`), `SceneError` | Topology failures are values; a cycle is rejected without mutating the graph; the wrapper translates status codes and the graph is freed exactly once in `Drop` |
| `mikan-input-sys` | `InputStatus` (`OK`, `BAD_ARGUMENT`, `NOT_FOUND`, `CONFLICT`, `UNKNOWN_CONTEXT`) plus `OVERFLOW` | No panics across the boundary: every entry point validates and returns a status code; a full event buffer drops the newest event and counts it, and allocation failure is fatal because the module builds without exceptions |
| `mikan-script` | `ScriptError` (`Load`, `Compile`, `MissingFunction`, `Runtime`, `InvalidReturn`, `Vm`) | Load-time failures are fatal to the caller; the engine does not decide what happens next — each caller does (the demo exits on load errors and logs-once idles on run-time errors) |
| `mikan-platform` | `PlatformError` (+ `PlatformError::app` for application errors) | Fatal errors stop the loop, run `shutdown`, and reach `main` |
| `mikan-render` | `RenderError` | Recoverable swap-chain states are *not* errors; unrecoverable ones are |
| `mikan` | `Game::Error` (bounded by `From<RenderError>`) | Renderer operations return `RenderError` so `?` composes; renderer startup failures are labeled and wrapped with `PlatformError::app` |
| `demo2d` | `DemoError` (wraps `ScriptError`) | Converted by the facade into `PlatformError`; one channel carries all failures to `main` |

Rules:

- Recoverable conditions are values (`FrameOutcome`), not exceptions or logged
  warnings, so callers must decide what to do.
- Unrecoverable conditions are returned, logged once at `error`, and produce a
  non-zero exit status.
- `unwrap`/`expect` are not used on recoverable paths. The remaining uses are in
  tests, where a panic is the assertion.
- No global mutable state; the only shared mutable value is the device-lost flag,
  which is a `Mutex<Option<String>>` written by the `wgpu` callback thread and
  read by the render thread.

## 8. Cross-platform considerations

- The engine targets Windows, Linux (X11 and Wayland), and macOS.
- Window creation goes through `winit`, which handles the platform differences.
  `ActiveEventLoop`, `Resumed`/`Suspended`, and `Occluded` are used rather than
  assumed, so mobile-style lifecycle events do not surprise the runner.
- The renderer goes through `wgpu` and restricts itself to the WebGPU-default
  feature set (`Features::empty()`, `Limits::default()`), which keeps it portable
  across Vulkan, Metal, DX12, and GLES.
- Platform-specific code is currently **absent** rather than hidden behind
  `#[cfg]` blocks, so there is nothing to audit yet.
- macOS and Windows builds are compile-verified only, and only in CI; the
  currently verified runtime is Linux/X11 with a software Vulkan device. See
  ADR 0005 for the validation policy and `DEVELOPMENT.md` for the commands to
  verify on a machine with a real GPU.

## 9. Native interoperability boundaries

Three first-party native subsystems and one vendored native runtime exist,
introduced by explicit decision:

- **The C99 collision kernel** (`crates/collision-sys`), wrapped by the
  safe `mikan-collision`. One batch function over plain `#[repr(C)]` data; the
  kernel keeps no state, allocates nothing, and cannot fail. It is one of the
  crates where `unsafe` is allowed (a workspace lint override whose reason is
  recorded in the crate root); the wrapper and every consumer are safe Rust.
  ADR 0010 records the decision, including the honest benchmark result: parity
  with the Rust reference, no speedup claimed.
- **The C transform kernel and C++ scene graph** (`crates/scene-sys`), wrapped
  by the safe `mikan-scene`. The kernel composes one `#[repr(C)]` transform from
  a parent and a local one; the graph holds the parent/children topology behind
  one opaque `extern "C"` handle, keeps no transforms, and returns a status code
  from every entry point. It is the second crate where `unsafe` is allowed and
  the first with hand-written C++ of Mikan's own. ADR 0012 records the decision,
  including the honest benchmark result: the native graph is *slower* than a
  Rust array fold because it carries real graph bookkeeping, and no speedup is
  claimed.
- **The C device backend and C++ action runtime** (`crates/input-sys`), wrapped
  by the safe `mikan-input`. The backend normalizes raw device events (and
  owns raw key/button state); the runtime maps bindings to actions across
  prioritized contexts and maintains each action's state machine. They never
  call each other — Rust drains one and feeds the other in a single batch per
  fixed step — and `reference` cross-checks the normalizer bit for bit. It is
  the third crate where `unsafe` is allowed. ADR 0013 records the decision,
  including the honest benchmark result: the C backend is slower than the Rust
  oracle and the full engine step stays under ≈40 ns/event, with no speedup
  claimed.
- **The vendored Luau VM (C++)**, compiled by `mlua`'s `vendored` feature and
  consumed through the safe `mikan-script`. Mikan writes no C++ glue; the
  surface is `extern "C"` functions generated by `mlua-sys`. Scripts are
  sandboxed (explicit library set, sandbox mode, memory limit), and script
  failures cross back as Rust `ScriptError`s carrying the script name and
  line. ADR 0011 records the decision.

The policy that governs these and any future native module — ownership, ABI,
error translation, thread safety, and the criteria that make a module worth
writing in C or C++ at all — is documented in [`ffi.md`](ffi.md); ADR 0004
remains the baseline and ADRs 0010, 0012, and 0013 supersede it for the
collision, scene, and input modules respectively. ADRs 0006–0009 record that
the four milestones before Phase 5 added none, with the reason.

## 10. Extension points

These are seams the current design leaves open. None of them are implemented.

- **Multi-window applications.** The facade owns one window and one renderer per
  process; an application that needs several windows composes the layers
  directly through `mikan_platform::App`, which remains public for this reason.
- **Sprite sorting / batching heuristics.** `plan_batches` merges only
  consecutive sprites sharing a texture and sampler mode; a sorter that groups
  across submission order (z layers, texture keys) is a local change once
  painter order can be expressed independently of submission order.
- **Asset pipeline.** `mikan-assets` decodes files one at a time; atlas
  packing, hot reload, and background loading are future work that plugs in
  front of `Renderer::create_texture` without changing the renderer.
- **ECS evolution.** `mikan-ecs` is the seam: archetype-dense storage, a
  parallel executor, change detection, or query filters are local additions
  once a profiled need exists. Nothing is stubbed today.
- **Frame graph / passes.** `render_frame` is the single place where pass
  recording happens; splitting it into ordered passes is a local change.
- **Native modules.** `docs/ffi.md` governs additions; the collision (ADR 0010),
  Luau (ADR 0011), scene (ADR 0012), and input (ADR 0013) modules are the
  worked examples of the whole path: design, tests, and records in
  `docs/STATUS.md`.
  ADR 0008 records why the 2D sprite-rendering milestone added none (its
  benchmark points at rasterization throughput, not at a boundary Rust
  expresses badly), and ADR 0009 why the ECS-lite milestone added none: its
  value is compile-time-enforced handle validity and non-aliasing over a wide,
  allocation-heavy data structure — exactly the properties an FFI boundary
  would weaken.
