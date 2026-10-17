//! Mikan platform layer: window lifecycle, input, and the application loop.
//!
//! The platform layer owns the operating-system-facing parts of the engine:
//!
//! - creating and maintaining the window ([`winit`] is the only windowing
//!   dependency, and it stays behind this crate),
//! - translating window events into [`InputState`],
//! - measuring frame time and scheduling fixed updates ([`mikan_core`]),
//! - running the [`App`] lifecycle hooks.
//!
//! It deliberately knows nothing about graphics APIs. Applications receive the
//! window handle in [`AppContext`] and create a renderer themselves, which keeps
//! the dependency direction one-way: `mikan-render` does not depend on
//! `mikan-platform`.
//!
//! # Example
//!
//! ```
//! use mikan_core::{EngineConfig, FrameOutcome};
//! use mikan_platform::{App, AppContext, PlatformError};
//!
//! struct Logging;
//!
//! impl App for Logging {
//!     fn init(&mut self, ctx: &mut AppContext<'_>) -> Result<(), PlatformError> {
//!         log::info!("started with {}x{}", ctx.config().window_width, ctx.config().window_height);
//!         Ok(())
//!     }
//!
//!     fn render(&mut self, ctx: &mut AppContext<'_>) -> Result<FrameOutcome, PlatformError> {
//!         if ctx.time().frame_count() > 100 {
//!             ctx.request_exit();
//!         }
//!         Ok(FrameOutcome::Presented)
//!     }
//! }
//!
//! // `mikan_platform::run(EngineConfig::default(), Logging)?;`
//! # Ok::<(), PlatformError>(())
//! ```

#![forbid(unsafe_code)]

mod app;
mod error;
mod input;
mod runner;

pub use app::{App, AppContext};
pub use error::PlatformError;
pub use input::{DeviceEvent, InputState};
pub use runner::run;

/// Runs an application with the given configuration until its window closes.
pub use runner::run as run_app;

// Re-exported so applications do not need a direct `winit` dependency for the
// common cases: reading the window size, and querying keyboard and mouse state.
pub use winit::dpi::{PhysicalPosition, PhysicalSize};
pub use winit::event::MouseButton;
pub use winit::keyboard::{KeyCode, PhysicalKey};
pub use winit::window::Window;
