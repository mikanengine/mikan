//! Mikan rendering layer.
//!
//! This crate owns everything between a window handle and a presented frame:
//!
//! - a [`wgpu`] instance, adapter, device, and queue,
//! - a swap chain surface with deliberate handling of every
//!   [`wgpu::CurrentSurfaceTexture`] outcome,
//! - a small renderer for flat, screen-space colored quads ([`Quad2d`]),
//! - optional single-frame readback ([`CaptureBuffer`]) for tests and tooling.
//!
//! It deliberately does **not** depend on `mikan-platform`: the renderer needs
//! only a window handle, expressed by the [`RenderWindow`] trait, so graphics
//! cannot reach into event handling and the dependency graph stays a DAG.
//!
//! # What this is not (yet)
//!
//! There is no camera, no texture or material support, no sprite batching, no
//! depth buffer, and no 3D pipeline. Quads are drawn with one instanced draw
//! call in submission order. Those are future milestones, not hidden features.
//!
//! # Example
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use mikan_core::Color;
//! # use mikan_render::{Frame, Quad2d, Renderer, RenderSettings};
//! # fn example(window: Arc<impl mikan_render::RenderWindow>, size: (u32, u32)) -> Result<(), mikan_render::RenderError> {
//! let mut renderer = Renderer::new(window, size, RenderSettings::default())?;
//! let quads = [Quad2d::new([100.0, 100.0], [64.0, 64.0], Color::WHITE)];
//! let frame = Frame::new(Color::default()).with_quads(&quads);
//! renderer.render_frame(&frame, None)?;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

mod capture;
mod error;
mod quad;
mod renderer;

pub use capture::CaptureBuffer;
pub use error::RenderError;
pub use quad::Quad2d;
pub use renderer::{RenderSettings, Renderer};

use core::fmt::Debug;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// A window (or window-like surface source) the renderer can present to.
///
/// The blanket implementation covers every window type that can provide raw
/// window and display handles, which includes `winit` windows and any other
/// windowing library that implements `raw-window-handle`. Applications pass an
/// `Arc<Window>` and it is coerced to `Arc<dyn RenderWindow>` at the call site.
///
/// The renderer keeps the handle alive for the lifetime of the surface, so the
/// window outlives every GPU resource created from it.
pub trait RenderWindow: HasWindowHandle + HasDisplayHandle + Debug + Send + Sync + 'static {}

impl<T> RenderWindow for T where
    T: HasWindowHandle + HasDisplayHandle + Debug + Send + Sync + 'static
{
}

/// A frame description: what to clear to, and what to draw.
///
/// Borrowed data (`quads`) must outlive the call to
/// [`Renderer::render_frame`].
#[derive(Debug)]
pub struct Frame<'a> {
    /// Color the swap chain image is cleared to before drawing.
    pub clear_color: mikan_core::Color,
    /// Screen-space quads, drawn in order, in physical pixels.
    pub quads: &'a [Quad2d],
}

impl<'a> Frame<'a> {
    /// Creates a frame that only clears the screen.
    #[must_use]
    pub fn new(clear_color: mikan_core::Color) -> Self {
        Self {
            clear_color,
            quads: &[],
        }
    }

    /// Attaches the quads to draw.
    #[must_use]
    pub fn with_quads(mut self, quads: &'a [Quad2d]) -> Self {
        self.quads = quads;
        self
    }
}
