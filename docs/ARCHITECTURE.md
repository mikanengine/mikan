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
games/demo2d             application: composes platform + render, owns gameplay
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
- `Quad2d` / `Frame` — a flat colored rectangle in screen space, and the frame
  description: clear color plus the quads to draw.
- `CaptureBuffer` — optional single-frame readback (RGBA8) used by tests and
  tooling.

### `games/demo2d`

The application. It owns gameplay state (`Square`), reads input from and writes
outputs to the engine through the public APIs only, and contains the command-line
interface, the PPM writer, and the frame-time statistics.

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
                    | demo2d  |
                    +---------+
```

Rules that keep this acyclic and comprehensible:

- `mikan-core` depends on nothing in the workspace.
- `mikan-platform` and `mikan-render` never depend on each other.
- Applications may depend on all three.
- No crate depends on an application.

The renderer's window requirement is expressed as the `RenderWindow` trait rather
than a dependency on `mikan-platform`, which is why the two layers can stay
siblings. The application provides the window handle from `AppContext` when it
constructs the `Renderer`.

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
6. Build the quad pipeline and configure the surface (skipped while the window
   has zero area).

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

### Quad rendering

The only geometry primitive is `Quad2d`: center, size, and color in physical
pixels with the origin at the top-left corner and y growing downward, matching
cursor coordinates. All quads in a frame are drawn with **one instanced draw
call**: a static six-vertex unit square plus one instance per quad
(32 bytes: `center`, `size`, `color`). The instance buffer grows
geometrically when a frame needs more instances than it currently holds.

This is intentionally not a sprite renderer: there are no textures, no atlas, no
camera, and no batching heuristics, because nothing needs them yet.

## 7. Error-handling strategy

| Layer | Error type | Policy |
| --- | --- | --- |
| `mikan-core` | `ConfigError` | Validation at startup; no runtime panics for bad input |
| `mikan-platform` | `PlatformError` (+ `PlatformError::app` for application errors) | Fatal errors stop the loop, run `shutdown`, and reach `main` |
| `mikan-render` | `RenderError` | Recoverable swap-chain states are *not* errors; unrecoverable ones are |
| `demo2d` | `DemoError` | Wrapped into `PlatformError` so one channel carries all failures |

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

- **Textures and sprites.** `Quad2d` becomes an instance with an atlas index and
  UV rectangle; the quad pipeline gains a texture bind group. The instance
  layout is versioned by a unit test that checks attribute offsets, so a change
  that drifts from the shader fails the test suite.
- **Camera.** The viewport uniform generalizes to a view/projection matrix; the
  pixel-space vertex stage becomes a matrix multiply.
- **Engine facade.** A `mikan` crate can own the wiring that `demo2d` currently
  does in `App::init`, so applications stop composing platform and rendering
  manually.
- **ECS-lite.** `App::update` is the natural place to drive systems; the fixed
  timestep already provides a deterministic schedule.
- **Frame graph / passes.** `render_frame` is the single place where pass
  recording happens; splitting it into ordered passes is a local change.
- **Native modules.** `mikan-core` is the intended home for safe wrappers around
  small C modules (for example an arena allocator), behind the rules in
  `FFI.md`.
