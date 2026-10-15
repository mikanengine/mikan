//! Drives a [`App`] from a winit event loop.
//!
//! # Frame model
//!
//! Rendering is driven by `RedrawRequested`, not by a busy loop. When a frame is
//! presented or skipped the runner requests another redraw; when the renderer
//! reports [`FrameOutcome::Idle`] (occluded, minimized, zero-sized window) the
//! runner switches the event loop to `ControlFlow::Wait` and stops asking for
//! frames until the window becomes drawable again. A suspended or occluded
//! application therefore consumes no CPU while it cannot draw.
//!
//! # Timing model
//!
//! Each frame ticks [`Time`], which clamps the measured delta, and feeds it to
//! [`FixedTimestep`], which yields a bounded number of fixed updates plus an
//! interpolation alpha for the render call. The simulation rate is therefore
//! independent of the display refresh rate, and a single long frame cannot
//! trigger an unbounded catch-up loop.

use std::sync::Arc;

use mikan_core::{EngineConfig, FixedTimestep, Time};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::app::{App, AppContext};
use crate::error::PlatformError;
use crate::input::InputState;

/// Runs `app` on the current thread until the window closes.
///
/// This function never returns while the application is running; on desktop
/// platforms it must be called from the main thread because winit requires it.
///
/// # Errors
///
/// Returns [`PlatformError`] if the configuration is invalid, if the event loop
/// cannot be created, or if the application reports an error from one of its
/// callbacks. A fatal error logged at `error` level is followed by a clean
/// shutdown of the application.
pub fn run<A: App>(config: EngineConfig, app: A) -> Result<(), PlatformError> {
    config.validate()?;

    let event_loop = winit::event_loop::EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut runner = Runner::new(config, app);
    event_loop.run_app(&mut runner)?;

    match runner.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

struct Runner<A: App> {
    config: EngineConfig,
    app: A,
    window: Option<Arc<Window>>,
    input: InputState,
    time: Time,
    fixed: FixedTimestep,
    occluded: bool,
    suspended: bool,
    idle: bool,
    initialized: bool,
    exit_requested: bool,
    shutdown_done: bool,
    error: Option<PlatformError>,
}

impl<A: App> Runner<A> {
    fn new(config: EngineConfig, app: A) -> Self {
        let time = Time::with_max_delta(config.max_frame_delta);
        let fixed = FixedTimestep::new(config.fixed_timestep_hz, config.max_steps_per_frame);
        Self {
            config,
            app,
            window: None,
            input: InputState::new(),
            time,
            fixed,
            occluded: false,
            suspended: false,
            idle: false,
            initialized: false,
            exit_requested: false,
            shutdown_done: false,
            error: None,
        }
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), PlatformError> {
        let attributes = Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_inner_size(LogicalSize::new(
                self.config.window_width,
                self.config.window_height,
            ));
        let window = event_loop.create_window(attributes)?;
        log::info!(
            "window created: {}x{} physical pixels, scale factor {}",
            window.inner_size().width,
            window.inner_size().height,
            window.scale_factor()
        );
        self.window = Some(Arc::new(window));
        Ok(())
    }

    /// Builds a context borrowing the disjoint pieces of runner state a callback
    /// is allowed to see.
    fn context<'a>(
        window: &'a Arc<Window>,
        input: &'a InputState,
        config: &'a EngineConfig,
        time: &'a Time,
        alpha: f32,
        exit_requested: &'a mut bool,
    ) -> AppContext<'a> {
        AppContext::new(window, input, config, time, alpha, exit_requested)
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: PlatformError) {
        log::error!("fatal error: {error}");
        if self.error.is_none() {
            self.error = Some(error);
        }
        self.shutdown(event_loop);
    }

    fn shutdown(&mut self, event_loop: &ActiveEventLoop) {
        if self.shutdown_done {
            event_loop.exit();
            return;
        }
        self.shutdown_done = true;

        if let Some(window) = self.window.clone() {
            let alpha = self.fixed.alpha();
            let mut ctx = Self::context(
                &window,
                &self.input,
                &self.config,
                &self.time,
                alpha,
                &mut self.exit_requested,
            );
            self.app.shutdown(&mut ctx);
        }
        log::info!("shutting down after {} frames", self.time.frame_count());
        event_loop.exit();
    }

    /// Runs simulation updates and one render call for the current redraw.
    fn draw(&mut self, event_loop: &ActiveEventLoop) {
        if self.suspended || self.occluded {
            // Nothing is visible; do not simulate or draw until the window
            // reports that it is drawable again.
            self.idle = true;
            return;
        }

        let Some(window) = self.window.clone() else {
            return;
        };

        let delta = self.time.tick();
        let steps = self.fixed.advance(delta);
        let step = self.fixed.step();

        for _ in 0..steps {
            let alpha = self.fixed.alpha();
            let mut ctx = Self::context(
                &window,
                &self.input,
                &self.config,
                &self.time,
                alpha,
                &mut self.exit_requested,
            );
            if let Err(error) = self.app.update(&mut ctx, step) {
                self.fail(event_loop, error);
                return;
            }
            if self.exit_requested {
                self.shutdown(event_loop);
                return;
            }
        }

        let alpha = self.fixed.alpha();
        let mut ctx = Self::context(
            &window,
            &self.input,
            &self.config,
            &self.time,
            alpha,
            &mut self.exit_requested,
        );
        let outcome = match self.app.render(&mut ctx) {
            Ok(outcome) => outcome,
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };

        // Edge state belongs to this frame only; clear it once the application
        // has seen it in every fixed update and in the render call.
        self.input.end_frame();
        self.idle = !outcome.should_continue() || self.occluded;

        if self.exit_requested {
            self.shutdown(event_loop);
            return;
        }
        if !self.idle {
            window.request_redraw();
        }
    }
}

impl<A: App> ApplicationHandler for Runner<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.suspended {
            // A resume after a suspend keeps the existing window on desktop
            // platforms; platforms that destroy surfaces on suspend (Android)
            // are not supported yet.
            self.suspended = false;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            return;
        }

        if self.window.is_none()
            && let Err(error) = self.create_window(event_loop)
        {
            self.fail(event_loop, error);
            return;
        }

        if !self.initialized
            && let Some(window) = self.window.clone()
        {
            self.initialized = true;
            let alpha = self.fixed.alpha();
            let mut ctx = Self::context(
                &window,
                &self.input,
                &self.config,
                &self.time,
                alpha,
                &mut self.exit_requested,
            );
            if let Err(error) = self.app.init(&mut ctx) {
                self.fail(event_loop, error);
                return;
            }
        }

        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        log::info!("application suspended; pausing frame requests");
        self.suspended = true;
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => self.shutdown(event_loop),

            WindowEvent::Resized(size) => {
                if let Some(window) = self.window.clone() {
                    let alpha = self.fixed.alpha();
                    let mut ctx = Self::context(
                        &window,
                        &self.input,
                        &self.config,
                        &self.time,
                        alpha,
                        &mut self.exit_requested,
                    );
                    self.app.resize(&mut ctx, size);
                }
                self.idle = false;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                self.input
                    .handle_key(event.physical_key, event.state, event.repeat);
            }

            WindowEvent::MouseInput { state, button, .. } => {
                self.input.handle_mouse_button(button, state);
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.input.handle_cursor_moved(position);
            }

            WindowEvent::Focused(focused) => {
                if !focused {
                    log::debug!("window lost focus; releasing held input");
                }
                self.input.set_focused(focused);
            }

            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if occluded {
                    log::debug!("window occluded; pausing frame requests");
                } else {
                    self.idle = false;
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::RedrawRequested => self.draw(event_loop),

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.suspended || self.occluded || self.idle {
            // Nothing can be drawn: block until the operating system has
            // something to say instead of spinning.
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
        } else {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.shutdown(event_loop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mikan_core::FrameOutcome;
    use std::time::Duration;

    #[test]
    fn idle_frames_stop_the_redraw_loop() {
        assert!(!FrameOutcome::Idle.should_continue());
        assert!(FrameOutcome::Presented.should_continue());
        assert!(FrameOutcome::Skipped.should_continue());
    }

    #[test]
    fn runner_uses_bounded_timing_from_config() {
        let config = EngineConfig {
            fixed_timestep_hz: 50,
            max_steps_per_frame: 5,
            ..EngineConfig::default()
        };
        let runner = Runner::new(config, ());
        assert_eq!(runner.fixed.step(), Duration::from_millis(20));
        assert_eq!(runner.fixed.max_steps_per_frame(), 5);
    }

    impl App for () {}
}
