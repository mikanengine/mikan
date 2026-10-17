//! The application lifecycle: the trait an application implements and the
//! context it is given.

use std::sync::Arc;

use mikan_core::{EngineConfig, FrameOutcome, Time};
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::error::PlatformError;
use crate::input::InputState;

/// A Mikan application.
///
/// The platform runner owns the window, the event loop, input state, and timing,
/// and calls these hooks in a fixed order:
///
/// 1. [`App::init`] once, when the platform is ready to create a window.
/// 2. [`App::resize`] whenever the window's physical size changes.
/// 3. [`App::update`] zero or more times per frame, always with the same fixed
///    `dt`.
/// 4. [`App::render`] once per frame, after all updates for that frame.
/// 5. [`App::shutdown`] once, on exit.
///
/// Every method has a default implementation, so an application implements only
/// what it needs.
pub trait App {
    /// Called once the window exists and graphics resources can be created.
    ///
    /// # Errors
    ///
    /// Returning an error aborts startup and exits the process with a failure
    /// status.
    fn init(&mut self, ctx: &mut AppContext<'_>) -> Result<(), PlatformError> {
        let _ = ctx;
        Ok(())
    }

    /// Called when the window's drawable size changes, including once with the
    /// initial size before the first frame.
    fn resize(&mut self, ctx: &mut AppContext<'_>, size: PhysicalSize<u32>) {
        let _ = (ctx, size);
    }

    /// Advances the simulation by exactly `dt`.
    ///
    /// `dt` is the fixed timestep, never a measured frame delta, so simulation
    /// remains deterministic with respect to input. A single frame may call this
    /// several times to catch up, bounded by
    /// [`EngineConfig::max_steps_per_frame`].
    ///
    /// # Errors
    ///
    /// Returning an error stops the event loop and exits the process with a
    /// failure status.
    fn update(
        &mut self,
        ctx: &mut AppContext<'_>,
        dt: core::time::Duration,
    ) -> Result<(), PlatformError> {
        let _ = (ctx, dt);
        Ok(())
    }

    /// Draws and presents one frame.
    ///
    /// The returned [`FrameOutcome`] tells the runner whether drawing should
    /// continue immediately ([`FrameOutcome::Presented`],
    /// [`FrameOutcome::Skipped`]) or pause until the next window event
    /// ([`FrameOutcome::Idle`]).
    ///
    /// # Errors
    ///
    /// Returning an error stops the event loop and exits the process with a
    /// failure status.
    fn render(&mut self, ctx: &mut AppContext<'_>) -> Result<FrameOutcome, PlatformError> {
        let _ = ctx;
        Ok(FrameOutcome::Presented)
    }

    /// Called once when the application is about to stop, whether because the
    /// user closed the window or because a fatal error occurred.
    fn shutdown(&mut self, ctx: &mut AppContext<'_>) {
        let _ = ctx;
    }
}

/// Read-only engine state handed to application callbacks.
///
/// The context is valid only for the duration of the callback. It intentionally
/// exposes no way to mutate engine state other than requesting exit and
/// draining the input event log; resources owned by the application (such as a
/// renderer) live in the application itself.
pub struct AppContext<'a> {
    window: &'a Arc<Window>,
    input: &'a mut InputState,
    config: &'a EngineConfig,
    time: &'a Time,
    alpha: f32,
    exit_requested: &'a mut bool,
}

impl<'a> AppContext<'a> {
    pub(crate) fn new(
        window: &'a Arc<Window>,
        input: &'a mut InputState,
        config: &'a EngineConfig,
        time: &'a Time,
        alpha: f32,
        exit_requested: &'a mut bool,
    ) -> Self {
        Self {
            window,
            input,
            config,
            time,
            alpha,
            exit_requested,
        }
    }

    /// The application window.
    ///
    /// The handle is reference counted, so an application may clone it and keep
    /// it alive for as long as it needs (for example to create a render surface).
    #[must_use]
    pub fn window(&self) -> &Arc<Window> {
        self.window
    }

    /// Current input state.
    #[must_use]
    pub fn input(&self) -> &InputState {
        self.input
    }

    /// Movable input state, for draining the per-frame device event log.
    ///
    /// The runner records every raw device event in [`InputState::take_events`]
    /// order; an input system processes that stream in the first fixed update
    /// of each frame, so all lower layers see exactly one batch per step.
    #[must_use]
    pub fn input_mut(&mut self) -> &mut InputState {
        self.input
    }

    /// Engine configuration this application was started with.
    #[must_use]
    pub fn config(&self) -> &EngineConfig {
        self.config
    }

    /// Frame timing.
    #[must_use]
    pub fn time(&self) -> &Time {
        self.time
    }

    /// Interpolation factor in `[0.0, 1.0)` between the previous and current
    /// fixed update.
    ///
    /// Meaningful in [`App::render`]: rendering at
    /// `lerp(previous_state, current_state, alpha)` produces smooth motion even
    /// though the simulation runs at a fixed rate.
    #[must_use]
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// Asks the runner to shut down cleanly after the current callback returns.
    ///
    /// Anything already scheduled for this frame (remaining fixed updates and
    /// the render call) still runs; the window is then closed and
    /// [`App::shutdown`] is called.
    pub fn request_exit(&mut self) {
        *self.exit_requested = true;
    }
}
