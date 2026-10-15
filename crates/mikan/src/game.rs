//! The application lifecycle at the facade level: the [`Game`] trait and the
//! [`Engine`] handle its hooks receive.

use core::time::Duration;

use mikan_core::FrameOutcome;
use mikan_platform::{AppContext, PhysicalSize};
use mikan_render::{CaptureBuffer, Frame, RenderError, Renderer};

/// A Mikan game, driven by [`run`](crate::run).
///
/// The facade owns the renderer and the engine loop; the game owns gameplay.
/// Hooks are called in the same order as [`mikan_platform::App`]:
///
/// 1. [`Game::init`] once, after the renderer exists.
/// 2. [`Game::resize`] whenever the drawable size changes, after the renderer
///    has taken the new size.
/// 3. [`Game::update`] zero or more times per frame, always with the same fixed
///    `dt`.
/// 4. [`Game::render`] once per frame, after all updates for that frame.
/// 5. [`Game::shutdown`] once, if and only if `init` was called — including
///    when `init` returned an error.
///
/// Every method has a default implementation, so a game implements only what it
/// needs.
pub trait Game {
    /// The application's error type.
    ///
    /// Engine operations return [`RenderError`], and this bound makes `?` work
    /// inside the hooks. The standard shape is a transparent variant with
    /// `#[from]`; a game whose only possible failures are engine failures can
    /// use [`RenderError`] itself.
    ///
    /// # Example
    ///
    /// ```
    /// use mikan::{Engine, FrameOutcome, Game, RenderError};
    ///
    /// #[derive(Debug, thiserror::Error)]
    /// enum GameError {
    ///     #[error(transparent)]
    ///     Render(#[from] RenderError),
    /// }
    ///
    /// struct MyGame;
    ///
    /// impl Game for MyGame {
    ///     type Error = GameError;
    ///
    ///     fn render(&mut self, engine: &mut Engine<'_, '_>) -> Result<FrameOutcome, GameError> {
    ///         let frame = mikan::Frame::new(mikan::Color::default());
    ///         Ok(engine.render_frame(&frame, None)?)
    ///     }
    /// }
    /// ```
    type Error: std::error::Error + Send + Sync + From<RenderError> + 'static;

    /// Called once, after the renderer exists and before the first frame.
    ///
    /// The renderer can be queried and asked for textures here; the window and
    /// the configured timing are available through [`Engine::ctx`].
    ///
    /// # Errors
    ///
    /// Returning an error aborts startup and exits the process with a failure
    /// status. [`Game::shutdown`] still runs, so resources acquired earlier in
    /// this hook are released.
    fn init(&mut self, engine: &mut Engine<'_, '_>) -> Result<(), Self::Error> {
        let _ = engine;
        Ok(())
    }

    /// Called when the drawable size changes, after [`Renderer::resize`] has
    /// been forwarded.
    ///
    /// `engine.renderer().size()` already reports `size`. A game that lays out
    /// against the window bounds usually does not need this hook at all: the
    /// current size is available anywhere through the engine.
    fn resize(&mut self, engine: &mut Engine<'_, '_>, size: PhysicalSize<u32>) {
        let _ = (engine, size);
    }

    /// Advances the simulation by exactly `dt`.
    ///
    /// `dt` is the fixed timestep, never a measured frame delta. A single frame
    /// may call this several times to catch up, bounded by
    /// [`EngineConfig::max_steps_per_frame`](mikan_core::EngineConfig::max_steps_per_frame).
    ///
    /// # Errors
    ///
    /// Returning an error stops the event loop and exits the process with a
    /// failure status.
    fn update(&mut self, engine: &mut Engine<'_, '_>, dt: Duration) -> Result<(), Self::Error> {
        let _ = (engine, dt);
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
    fn render(&mut self, engine: &mut Engine<'_, '_>) -> Result<FrameOutcome, Self::Error> {
        let _ = engine;
        Ok(FrameOutcome::Presented)
    }

    /// Called once when the application is about to stop, whether because the
    /// user closed the window or because a fatal error occurred.
    fn shutdown(&mut self, engine: &mut Engine<'_, '_>) {
        let _ = engine;
    }
}

/// The engine handle passed to every [`Game`] hook.
///
/// It bundles the platform context — window, input, configuration, frame time,
/// interpolation alpha, and exit requests — with the renderer the facade
/// created. Like [`AppContext`], it is valid only for the duration of the hook
/// that received it.
pub struct Engine<'a, 'ctx> {
    ctx: &'a mut AppContext<'ctx>,
    renderer: &'a mut Renderer,
}

impl<'a, 'ctx> Engine<'a, 'ctx> {
    /// Builds a handle from the pieces the runner owns. Games receive `Engine`
    /// values from the facade; they never construct them.
    pub(crate) fn new(ctx: &'a mut AppContext<'ctx>, renderer: &'a mut Renderer) -> Self {
        Self { ctx, renderer }
    }

    /// The platform context: window, input snapshot, configuration, frame
    /// time, interpolation alpha, and `request_exit`.
    #[must_use]
    pub fn ctx(&mut self) -> &mut AppContext<'ctx> {
        self.ctx
    }

    /// The renderer, for queries and texture creation.
    ///
    /// Deliberately `&Renderer`: frame submission goes through
    /// [`Engine::render_frame`], so the renderer's mutable state is reachable
    /// only through the frame call.
    #[must_use]
    pub fn renderer(&self) -> &Renderer {
        self.renderer
    }

    /// Renders and presents one frame through the engine's renderer.
    ///
    /// `capture` behaves exactly as in [`Renderer::render_frame`]: when set,
    /// the presented image is copied back to CPU memory before presentation,
    /// for tests, tooling, and screenshots.
    ///
    /// # Errors
    ///
    /// Fails under the same conditions as [`Renderer::render_frame`]; the error
    /// converts into the game's error type through the [`Game::Error`] bound.
    pub fn render_frame(
        &mut self,
        frame: &Frame<'_>,
        capture: Option<&mut CaptureBuffer>,
    ) -> Result<FrameOutcome, RenderError> {
        self.renderer.render_frame(frame, capture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn games_need_only_a_conforming_error_type() {
        #[derive(Debug, thiserror::Error)]
        #[error("custom failure")]
        struct CustomError;

        impl From<RenderError> for CustomError {
            fn from(_: RenderError) -> Self {
                Self
            }
        }

        struct Custom;
        impl Game for Custom {
            type Error = CustomError;
        }

        struct Plain;
        impl Game for Plain {
            type Error = RenderError;
        }

        // Both satisfy the trait, so the default hooks cover a game that only
        // declares its error type, and `RenderError` itself is accepted through
        // the blanket `From<T> for T` implementation.
        fn assert_game<G: Game>() {}
        assert_game::<Custom>();
        assert_game::<Plain>();
    }
}
