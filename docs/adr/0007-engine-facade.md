# 0007. An engine facade crate for application composition

- Status: Accepted
- Date: 2026-10-15

## Context

The roadmap's next milestone is "an engine facade crate (`mikan`) that wires
platform and rendering together so applications stop doing it by hand". Today
`demo2d` does that wiring itself: it creates the `Renderer` in `App::init` from
`AppContext::window()`, forwards every resize to `Renderer::resize`, converts
every `RenderError` into its own error type, and stores the renderer as an
`Option` because `App::init` is the first hook that can create it. Every
application would repeat that code, and each repetition is a place to get
renderer lifetime, resize forwarding, or error translation subtly wrong.

Several decisions are genuinely open:

1. **What the facade owns.** The event loop, timing, input, and window
   lifecycle already live in `mikan-platform`; the facade must compose them, not
   grow a second runtime beside them.
2. **How an application plugs in.** A new lifecycle trait, or extending
   `mikan_platform::App`?
3. **One error channel.** `PlatformError` cannot gain a render variant without
   making `mikan-platform` depend on `mikan-render`, which would break the
   one-way dependency rule of ADR 0002 and ADR 0003 that keeps event handling
   and graphics independent.
4. **What the callbacks may do to the renderer**, and with which mutability.
5. **Does this milestone add C or C++?** The same question ADR 0006 answered for
   the sprites/camera milestone. ADR 0004 requires a *measured* need that clears
   all five criteria in `docs/ffi.md`. There is still no profiling data, no
   third-party native library being integrated, and no interface obligation; a
   facade is composition and error plumbing, the least plausible place for
   native code.

## Decision

1. **A `Game` trait and `mikan::run`, layered on the existing runner.** `Game`
   mirrors the `App` lifecycle hook for hook (`init`, `resize`, `update`,
   `render`, `shutdown`) and is driven by a private adapter that implements
   `mikan_platform::App` and is passed, unchanged, to `mikan_platform::run`.
   There is one event loop, one timing model, and one lifecycle: the facade
   changes who owns the renderer, not how frames are scheduled.
2. **The facade owns the renderer's lifecycle.** The adapter creates the
   `Renderer` from `AppContext::window()` and `EngineConfig` (window size and
   `vsync`) before `Game::init`, forwards every resize to `Renderer::resize`
   *before* the game's resize hook (so `engine.renderer().size()` is already
   current), and drops the renderer when it is dropped. `Game::init` is
   guaranteed to run after the renderer exists, and `Game::shutdown` runs if and
   only if `Game::init` was invoked — including when `init` returned an error,
   so a partially initialized game still gets its cleanup hook. This is why the
   adapter keeps the renderer even when `Game::init` fails.
3. **Renderer operations return `RenderError`; the game's error type absorbs
   them.** `Game::Error` is bounded by `From<RenderError>`, so `?` composes with
   a game error declared as `#[error(transparent)] Render(#[from] RenderError)`.
   Renderer *creation* failures cannot travel through the game's type (the game
   never ran), so the facade reports them as an application-level error labeled
   "could not initialize the renderer: …" via `PlatformError::app`. `run` keeps
   returning `PlatformError`, so `main` keeps one error channel and the
   dependency graph keeps its direction.
4. **Callbacks receive `Engine`: the platform context plus the renderer.**
   `Engine::ctx()` exposes `AppContext` unchanged (window, input, time, alpha,
   exit requests). Renderer access is deliberately asymmetric:
   `Engine::renderer()` returns `&Renderer` (queries and texture creation), and
   frame submission goes through `Engine::render_frame`, so `&mut Renderer`
   never escapes the facade and rendering stays the only mutable step.
5. **The facade is the only crate an application has to depend on**, but it
   does not hide the layers: the crates remain reachable as
   `mikan::mikan_core`, `mikan::mikan_platform`, and `mikan::mikan_render`, and
   `mikan_platform::App` stays a public, supported way to wire the layers
   manually for the cases a single-window facade does not fit.
6. **No native module in this milestone.** The ADR 0004 criteria remain unmet,
   so the facade is Rust-only; this record exists so the finding is explicit
   rather than an omission.

## Consequences

Positive:

- `demo2d` loses its renderer field, its resize hook, and its error conversions;
  what remains in the demo is gameplay, CLI, and explicit render calls.
- Renderer lifetime, resize forwarding, and startup-error labeling are
  implemented once, and the existing runtime checks (smoke, input, camera) now
  exercise them — a facade refactor has to keep the expected pixels intact.
- Applications depend on one crate and one import list; the engine's public
  surface is described in one place.

Negative:

- Two lifecycle traits coexist (`mikan_platform::App` and `mikan::Game`). The
  facade documents which one it drives, and `App` stays public on purpose; the
  overlap is real and is the price of keeping the layers usable directly.
- `Game::Error` places a bound on application error types. An application that
  cannot convert from `RenderError` cannot use the facade, only the layers.
- One more crate for what is, today, a thin adapter. Accepted because the wiring
  it removes is the kind that accumulates subtly wrong variations per
  application.
- Facade failures gain one more Display layer ("application error: could not
  initialize the renderer: …") and callbacks sit one trait indirection away from
  the runner.
- The facade owns exactly one window and one renderer per process; multi-window
  support is a future decision, not an extension point of this design.
