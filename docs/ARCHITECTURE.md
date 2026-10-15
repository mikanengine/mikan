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
- An ECS, job system, or custom allocator.
- 3D rendering, PBR, shadows, or post-processing.
- Native C/C++ modules without a concrete, documented justification (ADR 0004).

## 2. Crate responsibilities

```
crates/mikan-core        no dependencies on windowing or graphics
crates/mikan-platform    winit is its only OS dependency; no graphics
crates/mikan-render      wgpu is its only graphics dependency; no event handling
crates/mikan             facade: one lifecycle that composes the platform and render layers
games/demo2d             application: implements the facade's Game trait, owns gameplay
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
  transformation and a sanitizing `effective_zoom`; no graphics dependency, so
  gameplay math and rendering share one definition.
- `Color` — linear RGBA, deliberately independent of `wgpu::Color`.
- `init_logging` — one-shot `env_logger` initialization honoring `RUST_LOG`.

### `mikan-platform`

Everything that touches the operating system's event and input model.

- `App` — the lifecycle trait: `init`, `resize`, `update`, `render`, `shutdown`.
- `AppContext` — read-only access to the window handle, input snapshot, config,
  frame time, and interpolation alpha, plus `request_exit`.
- `InputState` — keyboard and mouse state with held and per-frame edge queries,
  cursor delta, and focus handling that releases keys when focus is lost.
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
  through an atlas-ready UV rectangle, and the frame description: clear color,
  optional camera, and the sprites to draw.
- `Texture` / `TextureDescriptor` — textures uploaded from linear RGBA8 pixels
  and validated before any GPU resource is created; a texture is usable only
  with the `Renderer` that created it.
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

### `games/demo2d`

The application. It implements the facade's `Game` trait, owns gameplay state
(`Square`), and contains the command-line interface, the procedural atlas, the
PPM writer, and the frame-time statistics. It depends on `mikan` only; the
platform and render layers reach it through the facade's re-exports.

## 3. Dependency direction

```
                 +---------------+
                 |  mikan-core   |   (no deps beyond log/env_logger/thiserror)
                 +-------+-------+
                         |
         +---------------+---------------+
         |                               |
 +-------+---------+            +--------+--------+
 | mikan-platform  |            |  mikan-render   |
 | (winit)         |            |  (wgpu)         |
 +-------+---------+            +--------+--------+
         |                               |
         +---------------+---------------+
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

- `mikan-core` depends on nothing in the workspace.
- `mikan-platform` and `mikan-render` never depend on each other.
- `mikan` may depend on all three; it re-exports them so applications need one
  dependency. Applications that need custom wiring can still use
  `mikan_platform::App` directly.
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

The geometry primitive is `Sprite`: center, size, color, an optional texture,
and a UV rectangle, all in world units. A frame renders through an optional
`Camera2d`; without one, `Camera2d::pixel_space` maps world units 1:1 to
physical pixels with the origin at the top-left corner, reproducing the
pre-camera behavior exactly. Every frame is drawn with:

- one static six-vertex unit square shared by all sprites,
- one instance per sprite (48 bytes: `center`, `size`, `color`, `uv`),
- one instanced draw call per **consecutive run of sprites that share a
  texture** — painter order equals submission order, sprites are never
  reordered, and solid sprites share a 1×1 white fallback texture,
- one camera uniform (a `mat4x4` view/projection matrix built from
  `Camera2d`), and one texture bind group per texture.

The instance buffer grows geometrically when a frame needs more instances than
it currently holds. Texture descriptors are validated (non-zero, within
`max_texture_dimension_2d`, exact data length) before any GPU resource is
created, and textures are linear RGBA8 with a single mip level, uploaded from
memory — there is no image loader yet.

## 7. Error-handling strategy

| Layer | Error type | Policy |
| --- | --- | --- |
| `mikan-core` | `ConfigError` | Validation at startup; no runtime panics for bad input |
| `mikan-platform` | `PlatformError` (+ `PlatformError::app` for application errors) | Fatal errors stop the loop, run `shutdown`, and reach `main` |
| `mikan-render` | `RenderError` | Recoverable swap-chain states are *not* errors; unrecoverable ones are |
| `mikan` | `Game::Error` (bounded by `From<RenderError>`) | Renderer operations return `RenderError` so `?` composes; renderer startup failures are labeled and wrapped with `PlatformError::app` |
| `demo2d` | `DemoError` | Converted by the facade into `PlatformError`; one channel carries all failures to `main` |

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

There is **no FFI in the engine today**. No C or C++ module is compiled, linked,
or wrapped, and no crate depends on a native library beyond what `wgpu`, `winit`,
and `env_logger` bring in through the Rust ecosystem.

The policy that governs any future native module — ownership, ABI, error
translation, thread safety, and the criteria that make a module worth writing in
C or C++ at all — is documented in [`ffi.md`](ffi.md) and ADR 0004. The engine
must never depend on an unfinished native module: native code arrives with its
own tests and a safe wrapper, or it does not arrive.

## 10. Extension points

These are seams the current design leaves open. None of them are implemented.

- **Multi-window applications.** The facade owns one window and one renderer per
  process; an application that needs several windows composes the layers
  directly through `mikan_platform::App`, which remains public for this reason.
- **Sprite sorting / batching heuristics.** `plan_batches` currently merges
  only consecutive sprites sharing a texture; a sorter that groups across
  submission order (z layers, texture keys) is a local change once painter
  order can be expressed independently of submission order.
- **Image loading.** `Renderer::create_texture` takes decoded linear RGBA8
  pixels; an asset step that decodes files (and decides between sRGB and linear
  formats) plugs in front of it without changing the renderer.
- **ECS-lite.** `App::update` is the natural place to drive systems; the fixed
  timestep already provides a deterministic schedule.
- **Frame graph / passes.** `render_frame` is the single place where pass
  recording happens; splitting it into ordered passes is a local change.
- **Native modules.** `mikan-core` is the intended home for safe wrappers around
  small C modules (for example an arena allocator), behind the rules in
  `ffi.md`.
