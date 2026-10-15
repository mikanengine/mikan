//! Mikan core: platform-independent engine fundamentals.
//!
//! This crate has no dependency on windowing or graphics libraries. It provides
//! the small set of types the rest of the engine agrees on:
//!
//! - [`EngineConfig`]: configuration shared by the platform and render layers.
//! - [`Time`]: frame timing.
//! - [`FixedTimestep`]: fixed update scheduling with bounded catch-up.
//! - [`FrameOutcome`]: what happened to a frame presented by a renderer.
//! - [`Color`]: linear RGBA color.
//! - [`Camera2d`]: which part of 2D world space is visible.
//! - [`init_logging`]: one-shot logging initialization.
//!
//! Keeping these types here means `mikan-platform` and `mikan-render` never need
//! to depend on each other to share vocabulary.

#![forbid(unsafe_code)]

pub mod camera;
pub mod config;
pub mod error;
pub mod frame;
pub mod logging;
pub mod time;

pub use camera::Camera2d;
pub use config::{Color, EngineConfig};
pub use error::ConfigError;
pub use frame::FrameOutcome;
pub use logging::init_logging;
pub use time::{FixedTimestep, Time};
