//! The facade runner: adapts a [`Game`] to the platform's [`App`] lifecycle.

use std::sync::Arc;
use std::time::Duration;

use mikan_core::{EngineConfig, FrameOutcome};
use mikan_platform::{App, AppContext, PhysicalSize, PlatformError};
use mikan_render::{RenderError, RenderSettings, RenderWindow, Renderer};
use thiserror::Error;

use crate::game::{Engine, Game};

/// Runs `game` until its window closes.
///
/// Equivalent to [`mikan_platform::run`] with an adapter in between that:
///
/// 1. creates the [`Renderer`] from the window and the configured vsync mode
///    before [`Game::init`] runs,
/// 2. forwards every resize to the renderer before calling [`Game::resize`],
/// 3. wraps game errors in [`PlatformError::app`] so one channel carries every
///    failure.
///
/// # Errors
///
/// Returns [`PlatformError`] under the same conditions as
/// [`mikan_platform::run`]: invalid configuration, event-loop or window
/// creation failure, renderer initialization failure (reported as an
/// application error labeled "could not initialize the renderer"), or an error
/// returned from a game hook.
pub fn run<G: Game>(config: EngineConfig, game: G) -> Result<(), PlatformError> {
    mikan_platform::run(config, EngineApp::new(game))
}

/// Maps the engine configuration onto renderer settings.
fn render_settings(config: &EngineConfig) -> RenderSettings {
    RenderSettings {
        vsync: config.vsync,
    }
}

/// A renderer initialization failure.
///
/// The label matters: without it the error would reach `main` as a bare
/// `RenderError` behind "application error", which does not say which phase of
/// startup failed.
#[derive(Debug, Error)]
#[error("could not initialize the renderer: {0}")]
struct RendererStartup(#[from] RenderError);

/// The bridge between the facade lifecycle [`Game`] and the platform's [`App`].
///
/// `renderer` is `None` until [`EngineApp::init`] creates it, and `Some` from
/// then on. Those two facts are one fact: the renderer is created immediately
/// before [`Game::init`] is invoked, so "the renderer exists" means "the game's
/// init ran" — which is exactly when [`Game::shutdown`] must be delivered.
struct EngineApp<G: Game> {
    game: G,
    renderer: Option<Renderer>,
}

impl<G: Game> EngineApp<G> {
    fn new(game: G) -> Self {
        Self {
            game,
            renderer: None,
        }
    }
}

impl<G: Game> App for EngineApp<G> {
    fn init(&mut self, ctx: &mut AppContext<'_>) -> Result<(), PlatformError> {
        let window: Arc<dyn RenderWindow> = ctx.window().clone();
        let size = ctx.window().inner_size();
        let mut renderer = Renderer::new(
            window,
            (size.width, size.height),
            render_settings(ctx.config()),
        )
        .map_err(|error| PlatformError::app(RendererStartup::from(error)))?;

        // The renderer is kept even if `Game::init` fails: the platform
        // delivers `shutdown` exactly once, and a game that failed partway
        // through initialization may still hold resources to release.
        let result = self.game.init(&mut Engine::new(ctx, &mut renderer));
        self.renderer = Some(renderer);
        result.map_err(PlatformError::app)
    }

    fn resize(&mut self, ctx: &mut AppContext<'_>, size: PhysicalSize<u32>) {
        let Some(renderer) = self.renderer.as_mut() else {
            // The runner creates the window and calls `init` before delivering
            // any window event, so a resize cannot arrive first.
            return;
        };
        renderer.resize(size.width, size.height);
        self.game.resize(&mut Engine::new(ctx, renderer), size);
    }

    fn update(&mut self, ctx: &mut AppContext<'_>, dt: Duration) -> Result<(), PlatformError> {
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(());
        };
        self.game
            .update(&mut Engine::new(ctx, renderer), dt)
            .map_err(PlatformError::app)
    }

    fn render(&mut self, ctx: &mut AppContext<'_>) -> Result<FrameOutcome, PlatformError> {
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(FrameOutcome::Idle);
        };
        self.game
            .render(&mut Engine::new(ctx, renderer))
            .map_err(PlatformError::app)
    }

    fn shutdown(&mut self, ctx: &mut AppContext<'_>) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        self.game.shutdown(&mut Engine::new(ctx, renderer));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_settings_follow_the_configuration() {
        let vsync = EngineConfig {
            vsync: true,
            ..EngineConfig::default()
        };
        assert!(render_settings(&vsync).vsync);

        let uncapped = EngineConfig {
            vsync: false,
            ..EngineConfig::default()
        };
        assert!(!render_settings(&uncapped).vsync);
    }

    #[test]
    fn renderer_startup_failures_are_labeled_and_keep_their_cause() {
        let error = PlatformError::app(RendererStartup::from(RenderError::NoAdapter));
        let message = error.to_string();
        assert!(
            message.contains("could not initialize the renderer"),
            "{message}"
        );
        assert!(message.contains("no graphics adapter"), "{message}");
        assert!(std::error::Error::source(&error).is_some());
    }
}
