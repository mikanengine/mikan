# 0003. Windowing and rendering are separate layers joined by the application

- Status: Accepted
- Date: 2026-10-15

## Context

Windowing and rendering are entangled in most small engines: the renderer holds
the window, the event loop calls into the renderer, and input handling ends up
inside graphics code. That coupling causes predictable problems:

- the renderer cannot be tested without a window, and the window cannot be
  driven without a GPU;
- an event handling change can break rendering, and vice versa;
- replacing windowing or graphics libraries means rewriting both layers;
- application logic has no defined place to live.

Mikan already separates crates by responsibility (ADR 0001), which forces the
question into the open: what exactly crosses between the layers that own the
window and the GPU?

The relevant constraint is that `wgpu::Instance::create_surface` needs a window
handle, and `winit` windows implement `raw-window-handle`. So the renderer needs
*a window handle*, not `winit` itself.

## Decision

`mikan-platform` (windowing, input, event loop) and `mikan-render` (device,
swap chain, drawing) are sibling crates. Neither depends on the other; both
depend on `mikan-core` for shared vocabulary. The application wires them
together.

Concretely:

- **The renderer depends on `raw-window-handle`, not `winit`.** Its entry point
  takes `Arc<dyn RenderWindow>`, where `RenderWindow` is a blanket-implemented
  trait over `HasWindowHandle + HasDisplayHandle + Debug + Send + Sync`. A
  `winit` window, an SDL window, or a test double all qualify. The renderer keeps
  the `Arc` alive for the surface's lifetime, so the surface cannot outlive the
  window it presents to.
- **The platform layer never mentions graphics.** It owns the event loop, the
  window, and the input snapshot; it does not know that `wgpu` exists.
- **The application owns the composition.** It receives the window handle from
  `AppContext`, constructs the renderer itself, forwards resize events, and
  supplies the quads to draw each frame. `demo2d` is the reference example.
- **Shared frame vocabulary lives in `mikan-core`.** `FrameOutcome`
  (`Presented`/`Skipped`/`Idle`) is defined in the core crate so both layers can
  speak about frame results without depending on each other. The application loop
  uses it to decide whether to keep drawing or to wait for events.

## Consequences

Positive:

- The renderer is usable and testable whenever a window handle is available —
  including headless X servers with software rasterizers, which is how CI
  verifies it.
- Windowing and rendering can evolve independently. A future engine facade crate
  (`mikan`) can own the composition once more than one application needs it.
- The dependency graph stays acyclic and small: `mikan-core` is the only shared
  crate, and it has no platform or graphics dependencies at all.
- The boundary is a single, small trait, so it is visible in one file instead of
  being spread through both layers.

Negative:

- Applications write the wiring: create the renderer in `init`, forward
  `resize`, build a `Frame`, call `render_frame`. This is explicit but it is
  boilerplate, and it is currently duplicated nowhere only because there is
  exactly one application.
- Resize and occlusion information must be forwarded across the boundary by the
  application; a missed forward is a bug the compiler cannot catch.
- Two crates must be understood to reason about a frame, rather than one.
