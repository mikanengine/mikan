//! Mikan engine facade.
//!
//! This is the single crate an application has to depend on. It composes the
//! platform layer (window, event loop, input, timing) with the render layer
//! (device, swap chain, sprites) into one lifecycle:
//!
//! - the renderer is created once the window exists, from the window handle and
//!   the configured vsync mode, before [`Game::init`] runs,
//! - every window resize is forwarded to the renderer before the game sees it,
//! - frame submission goes through [`Engine::render_frame`], the only operation
//!   that needs mutable access to the renderer,
//! - engine failures and application failures travel through one error channel:
//!   [`run`] returns [`PlatformError`].
//!
//! The facade does not replace the layers. The event loop, timing, input, and
//! window lifecycle remain in [`mikan_platform`], driven by the same runner an
//! application can still use directly through [`mikan_platform::App`]; the
//! engine crates stay reachable as [`mikan_core`], [`mikan_platform`],
//! [`mikan_render`], [`mikan_assets`], [`mikan_ecs`], [`mikan_collision`], and
//! [`mikan_script`] for anything this crate does not re-export.
//!
//! # Example
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use mikan::{
//!     Engine, EngineConfig, Frame, FrameOutcome, Game, KeyCode, PhysicalKey, RenderError,
//! };
//!
//! /// The application error type must be convertible from `RenderError`; the
//! /// usual shape is a transparent variant with `#[from]`.
//! #[derive(Debug, thiserror::Error)]
//! enum GameError {
//!     #[error(transparent)]
//!     Render(#[from] RenderError),
//! }
//!
//! struct Mover {
//!     x: f32,
//! }
//!
//! impl Game for Mover {
//!     type Error = GameError;
//!
//!     fn update(&mut self, engine: &mut Engine<'_, '_>, dt: Duration) -> Result<(), GameError> {
//!         self.x += 120.0 * dt.as_secs_f32();
//!         if engine
//!             .ctx()
//!             .input()
//!             .is_key_just_pressed(PhysicalKey::Code(KeyCode::Escape))
//!         {
//!             engine.ctx().request_exit();
//!         }
//!         Ok(())
//!     }
//!
//!     fn render(&mut self, engine: &mut Engine<'_, '_>) -> Result<FrameOutcome, GameError> {
//!         let sprite = mikan::Sprite::solid([self.x, 100.0], [64.0, 64.0], mikan::Color::WHITE);
//!         let frame =
//!             Frame::new(mikan::Color::default()).with_sprites(std::slice::from_ref(&sprite));
//!         Ok(engine.render_frame(&frame, None)?)
//!     }
//! }
//!
//! fn main() -> Result<(), mikan::PlatformError> {
//!     mikan::run(EngineConfig::default(), Mover { x: 0.0 })
//! }
//! ```

#![forbid(unsafe_code)]

mod game;
mod runner;

pub use crate::game::{Engine, Game};
pub use crate::runner::run;

// The layers stay reachable for anything the curated re-exports below do not
// cover.
pub use mikan_assets;
pub use mikan_collision;
pub use mikan_core;
pub use mikan_ecs;
pub use mikan_platform;
pub use mikan_render;
pub use mikan_script;

// Curated re-exports so that an application needs no other dependency: the
// vocabulary of a frame, the platform context, asset loading, entity storage,
// collision queries, gameplay scripting, and the renderer entry points.
pub use mikan_assets::{Image, ImageError, SheetError, SheetFrame, SpriteSheet};
pub use mikan_collision::{Aabb, Broadphase, Contact, Pair, contacts};
pub use mikan_core::{Camera2d, Color, EngineConfig, FrameOutcome, Time, init_logging};
pub use mikan_ecs::{DeadEntity, Entity, Schedule, World};
pub use mikan_platform::{
    AppContext, InputState, KeyCode, PhysicalKey, PhysicalSize, PlatformError,
};
pub use mikan_render::{
    CaptureBuffer, Frame, FrameStats, RenderError, RenderSettings, Renderer, SamplerMode, Sprite,
    Texture, TextureDescriptor, TextureEncoding, UvRect,
};
pub use mikan_script::{ScriptEngine, ScriptError, StepInput, StepOutput};
