//! Mikan rendering layer.
//!
//! This crate owns everything between a window handle and a presented frame:
//!
//! - a [`wgpu`] instance, adapter, device, and queue,
//! - a swap chain surface with deliberate handling of every
//!   [`wgpu::CurrentSurfaceTexture`] outcome,
//! - textures uploaded from linear RGBA8 pixel data ([`Texture`]),
//! - a small renderer for [`Sprite`]s: flat colored or textured rectangles,
//!   drawn in order with one instanced draw call per run of sprites that share
//!   a texture, under an optional 2D [`Camera2d`](mikan_core::Camera2d),
//! - optional single-frame readback ([`CaptureBuffer`]) for tests and tooling.
//!
//! It deliberately does **not** depend on `mikan-platform`: the renderer needs
//! only a window handle, expressed by the [`RenderWindow`] trait, so graphics
//! cannot reach into event handling and the dependency graph stays a DAG.
//!
//! # What this is not (yet)
//!
//! There is no image loading, no atlas packing, no rotation, no depth buffer,
//! and no 3D pipeline. Textures have a single mip level and no sRGB variant.
//! Painting order equals submission order; sprites are never reordered to
//! reduce draw calls.
//!
//! # Example
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use mikan_core::{Camera2d, Color};
//! # use mikan_render::{Frame, Renderer, RenderSettings, Sprite, TextureDescriptor};
//! # fn example(window: Arc<impl mikan_render::RenderWindow>, size: (u32, u32)) -> Result<(), mikan_render::RenderError> {
//! let mut renderer = Renderer::new(window, size, RenderSettings::default())?;
//!
//! // Textures are uploaded as linear RGBA8 pixels.
//! let pixels = [255u8, 128, 0, 255];
//! let texture = renderer.create_texture(TextureDescriptor::new(1, 1, &pixels))?;
//!
//! let sprites = [
//!     Sprite::new([100.0, 100.0], [64.0, 64.0], &texture),
//!     Sprite::solid([200.0, 100.0], [32.0, 32.0], Color::WHITE),
//! ];
//! let camera = Camera2d::pixel_space([960.0, 600.0]);
//! let frame = Frame::new(Color::default())
//!     .with_camera(camera)
//!     .with_sprites(&sprites);
//! renderer.render_frame(&frame, None)?;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

mod capture;
mod error;
mod renderer;
mod sprite;
mod texture;

pub use capture::CaptureBuffer;
pub use error::RenderError;
pub use renderer::{RenderSettings, Renderer};
pub use sprite::{Sprite, UvRect};
pub use texture::{TEXTURE_FORMAT, Texture, TextureDescriptor};

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

/// A frame description: what to clear to, what view of the world to draw, and
/// which sprites to draw.
///
/// Borrowed data (`sprites`, and the textures they reference) must outlive the
/// call to [`Renderer::render_frame`].
#[derive(Debug)]
pub struct Frame<'a> {
    /// Color the swap chain image is cleared to before drawing.
    pub clear_color: mikan_core::Color,
    /// Camera to draw through, or `None` for pixel space: one world unit per
    /// physical pixel with the world origin at the top-left corner.
    pub camera: Option<mikan_core::Camera2d>,
    /// Sprites, drawn in order. Painter order equals submission order.
    pub sprites: &'a [Sprite<'a>],
}

impl<'a> Frame<'a> {
    /// Creates a frame that only clears the screen.
    #[must_use]
    pub fn new(clear_color: mikan_core::Color) -> Self {
        Self {
            clear_color,
            camera: None,
            sprites: &[],
        }
    }

    /// Draws the frame through `camera` instead of in pixel space.
    #[must_use]
    pub fn with_camera(mut self, camera: mikan_core::Camera2d) -> Self {
        self.camera = Some(camera);
        self
    }

    /// Attaches the sprites to draw.
    #[must_use]
    pub fn with_sprites(mut self, sprites: &'a [Sprite<'a>]) -> Self {
        self.sprites = sprites;
        self
    }
}
