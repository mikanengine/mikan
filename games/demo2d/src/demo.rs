//! The demo application: a square you can drive around with the keyboard.
//!
//! Everything gameplay-facing lives here. The demo implements
//! [`mikan_platform::App`], so it is driven by the engine's real runtime: fixed
//! updates timed by `mikan-core`, window and input handling from
//! `mikan-platform`, and rendering through `mikan-render`.
//!
//! Textures come from a procedural atlas (see [`crate::atlas`]); nothing is
//! loaded from disk yet.

use std::sync::Arc;
use std::time::Duration;

use mikan_core::{Camera2d, FrameOutcome};
use mikan_platform::{App, AppContext, KeyCode, PhysicalKey, PhysicalSize, PlatformError};
use mikan_render::{
    CaptureBuffer, Frame, RenderSettings, RenderWindow, Renderer, Sprite, Texture,
    TextureDescriptor,
};
use thiserror::Error;

use crate::atlas;
use crate::cli::{CameraMode, Cli};
use crate::ppm;
use crate::square::{Square, direction_from_keys};

/// Side length of the player square in pixels.
const SQUARE_SIZE: f32 = 96.0;
/// Player speed in pixels per second.
const SQUARE_SPEED: f32 = 420.0;
/// Edge length of the static marker drawn in the corner.
const MARKER_SIZE: f32 = 48.0;
/// Distance of the marker from the bottom-left corner.
const MARKER_MARGIN: f32 = 24.0;

/// Failures the demo itself can report.
#[derive(Debug, Error)]
pub enum DemoError {
    /// Rendering failed.
    #[error(transparent)]
    Render(#[from] mikan_render::RenderError),

    /// Writing a captured frame failed.
    #[error(transparent)]
    Image(#[from] ppm::PpmError),

    /// The requested frames were not presented within the timeout.
    #[error(
        "presented {presented} of {requested} requested frames within {timeout_secs}s; \
         the window or graphics device is not producing frames"
    )]
    FrameTimeout {
        /// Frames presented before giving up.
        presented: u64,
        /// Frames requested on the command line.
        requested: u64,
        /// Timeout in seconds.
        timeout_secs: u64,
    },

    /// Capture was requested, but the swap chain cannot be copied from.
    #[error("this surface does not support frame capture; omit --capture")]
    CaptureUnsupported,
}

/// Demo application state.
pub struct Demo {
    cli: Cli,
    renderer: Option<Renderer>,
    /// Atlas texture uploaded at startup, kept alive for the sprites that
    /// borrow it.
    atlas: Option<Texture>,
    square: Square,
    viewport: [f32; 2],
    presented: u64,
    capture: Option<CaptureBuffer>,
    captured: bool,
    capture_finalized: bool,
    frame_times: Vec<Duration>,
    idle_warned: bool,
}

impl Demo {
    /// Builds the demo from parsed command-line options.
    #[must_use]
    pub fn new(cli: Cli) -> Self {
        Self {
            capture: cli.capture.as_ref().map(|_| CaptureBuffer::new()),
            cli,
            renderer: None,
            atlas: None,
            square: Square::new([0.0, 0.0], SQUARE_SIZE, SQUARE_SPEED),
            viewport: [0.0, 0.0],
            presented: 0,
            captured: false,
            capture_finalized: false,
            frame_times: Vec::new(),
            idle_warned: false,
        }
    }

    /// Current movement direction from the held keys.
    fn direction(ctx: &AppContext<'_>) -> [f32; 2] {
        let input = ctx.input();
        let held = |key: KeyCode| input.is_key_pressed(PhysicalKey::Code(key));
        direction_from_keys((
            held(KeyCode::KeyA) || held(KeyCode::ArrowLeft),
            held(KeyCode::KeyD) || held(KeyCode::ArrowRight),
            held(KeyCode::KeyW) || held(KeyCode::ArrowUp),
            held(KeyCode::KeyS) || held(KeyCode::ArrowDown),
        ))
    }

    /// Sprites for this frame: the player square and a static corner marker,
    /// both sampling their tile of the atlas.
    ///
    /// The returned sprites borrow `atlas`, which is why it is a parameter
    /// rather than read from `self`: the render call borrows `self` mutably.
    fn sprites<'t>(&self, atlas: &'t Texture, alpha: f32) -> [Sprite<'t>; 2] {
        let center = self.square.interpolated_position(alpha);
        let size = self.square.size();
        let marker_center = [
            MARKER_MARGIN + MARKER_SIZE * 0.5,
            (self.viewport[1] - MARKER_MARGIN - MARKER_SIZE * 0.5).max(MARKER_SIZE * 0.5),
        ];
        [
            Sprite::new(center, [size, size], atlas).with_uv(atlas::tile_uv(atlas::PLAYER_TILE)),
            Sprite::new(marker_center, [MARKER_SIZE, MARKER_SIZE], atlas)
                .with_uv(atlas::tile_uv(atlas::MARKER_TILE)),
        ]
    }

    /// Camera for this frame, or `None` for the pixel-space view.
    fn camera(&self, alpha: f32) -> Option<Camera2d> {
        match self.cli.camera {
            CameraMode::None => None,
            // The camera centers on the interpolated player position, so the
            // player stays still on screen while the world scrolls.
            CameraMode::Follow => Some(Camera2d::new(
                self.square.interpolated_position(alpha),
                self.cli.zoom,
            )),
        }
    }

    /// Writes the most recent captured frame, if one was requested.
    ///
    /// The attempt is marked as finalized even when it fails, so a failed write
    /// is reported once (by the caller that could fail) instead of being retried
    /// during shutdown.
    fn write_capture(&mut self) -> Result<(), DemoError> {
        if self.capture_finalized {
            return Ok(());
        }
        let (Some(path), Some(buffer)) = (self.cli.capture.clone(), self.capture.as_ref()) else {
            return Ok(());
        };
        // Only write a frame that was actually captured: the buffer is resized
        // before the readback completes, so an aborted frame would otherwise be
        // written as a black image.
        if !self.captured {
            log::warn!("no frame has been captured yet; nothing written to {path:?}");
            return Ok(());
        }
        self.capture_finalized = true;
        ppm::write_ppm(&path, buffer.width(), buffer.height(), buffer.pixels())?;
        log::info!(
            "wrote {}x{} frame to {}",
            buffer.width(),
            buffer.height(),
            path.display()
        );
        Ok(())
    }

    /// Prints frame-time statistics collected during the run.
    fn print_stats(&self) {
        if !self.cli.stats || self.frame_times.is_empty() {
            return;
        }
        let mut times: Vec<f32> = self
            .frame_times
            .iter()
            .map(|delta| delta.as_secs_f32() * 1_000.0)
            .collect();
        times.sort_by(f32::total_cmp);
        let total: f32 = times.iter().sum();
        let mean = total / times.len() as f32;
        let percentile = |fraction: f32| times[((times.len() - 1) as f32 * fraction) as usize];
        println!(
            "frames: {} | mean {:.2} ms | p50 {:.2} ms | p95 {:.2} ms | max {:.2} ms | mean {:.1} fps",
            times.len(),
            mean,
            percentile(0.50),
            percentile(0.95),
            times[times.len() - 1],
            1_000.0 / mean.max(f32::EPSILON),
        );
    }
}

impl App for Demo {
    fn init(&mut self, ctx: &mut AppContext<'_>) -> Result<(), PlatformError> {
        let window: Arc<dyn RenderWindow> = ctx.window().clone();
        let size = ctx.window().inner_size();
        let renderer = Renderer::new(
            window,
            (size.width, size.height),
            RenderSettings {
                vsync: ctx.config().vsync,
            },
        )
        .map_err(|error| PlatformError::app(DemoError::from(error)))?;

        if self.cli.capture.is_some() && !renderer.supports_capture() {
            return Err(PlatformError::app(DemoError::CaptureUnsupported));
        }

        let atlas_data = atlas::build();
        let atlas = renderer
            .create_texture(TextureDescriptor::new(
                atlas_data.width,
                atlas_data.height,
                &atlas_data.pixels,
            ))
            .map_err(|error| PlatformError::app(DemoError::from(error)))?;

        let info = renderer.adapter_info();
        log::info!(
            "rendering with {} ({:?}), swap chain format {:?}",
            info.name,
            info.backend,
            renderer.surface_format()
        );
        log::info!(
            "textures: procedural atlas {}x{}, {} tiles of {}px",
            atlas_data.width,
            atlas_data.height,
            atlas::ATLAS_COLUMNS * atlas::ATLAS_ROWS,
            atlas::TILE_SIZE
        );
        log::info!("controls: WASD or arrow keys to move, Escape to quit");

        self.viewport = [size.width as f32, size.height as f32];
        self.square = Square::new(
            [self.viewport[0] * 0.5, self.viewport[1] * 0.5],
            SQUARE_SIZE,
            SQUARE_SPEED,
        );
        self.atlas = Some(atlas);
        self.renderer = Some(renderer);
        Ok(())
    }

    fn resize(&mut self, ctx: &mut AppContext<'_>, size: PhysicalSize<u32>) {
        self.viewport = [size.width as f32, size.height as f32];
        if let Some(renderer) = &mut self.renderer {
            renderer.resize(size.width, size.height);
        }
        let _ = ctx;
    }

    fn update(&mut self, ctx: &mut AppContext<'_>, dt: Duration) -> Result<(), PlatformError> {
        if ctx
            .input()
            .is_key_just_pressed(PhysicalKey::Code(KeyCode::Escape))
        {
            log::info!("escape pressed; quitting");
            ctx.request_exit();
            return Ok(());
        }

        let direction = Self::direction(ctx);
        let viewport = [self.viewport[0].max(1.0), self.viewport[1].max(1.0)];
        self.square.fixed_update(direction, dt, viewport);
        Ok(())
    }

    fn render(&mut self, ctx: &mut AppContext<'_>) -> Result<FrameOutcome, PlatformError> {
        if let Some(timeout) = self.cli.timeout
            && let Some(target) = self.cli.frames
            && self.presented < target
            && ctx.time().elapsed() > timeout
        {
            return Err(PlatformError::app(DemoError::FrameTimeout {
                presented: self.presented,
                requested: target,
                timeout_secs: timeout.as_secs(),
            }));
        }

        let alpha = ctx.alpha();
        let clear_color = ctx.config().clear_color;

        // Sprites borrow the atlas, so they are built before the renderer is
        // borrowed mutably; the borrows are disjoint fields of `self`.
        let Some(atlas) = self.atlas.as_ref() else {
            return Ok(FrameOutcome::Idle);
        };
        let sprites = self.sprites(atlas, alpha);
        let camera = self.camera(alpha);
        let mut frame = Frame::new(clear_color).with_sprites(&sprites);
        if let Some(camera) = camera {
            frame = frame.with_camera(camera);
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(FrameOutcome::Idle);
        };

        let capturing = self.capture.is_some();
        let outcome = renderer
            .render_frame(&frame, self.capture.as_mut())
            .map_err(|error| PlatformError::app(DemoError::from(error)))?;
        if capturing {
            self.captured = true;
        }

        match outcome {
            FrameOutcome::Presented => {
                self.presented += 1;
                if self.cli.stats {
                    self.frame_times.push(ctx.time().delta());
                }
            }
            FrameOutcome::Skipped => {}
            FrameOutcome::Idle => {
                if !self.idle_warned {
                    log::debug!("nothing to draw; waiting for the window to become drawable");
                    self.idle_warned = true;
                }
            }
        }

        if let Some(target) = self.cli.frames
            && self.presented >= target
        {
            log::info!("presented {target} frames; exiting");
            self.write_capture().map_err(PlatformError::app)?;
            ctx.request_exit();
        }

        Ok(outcome)
    }

    fn shutdown(&mut self, ctx: &mut AppContext<'_>) {
        // The interactive path has no frame budget, so the capture is written
        // here; failures can only be logged because shutdown cannot fail.
        if let Err(error) = self.write_capture() {
            log::error!("frame capture failed: {error}");
        }
        self.print_stats();

        // A single summary line makes smoke-test runs greppable: it states how
        // many frames were presented out of the frames the loop attempted, and
        // where the simulation ended up.
        let position = self.square.position();
        log::info!(
            "demo finished: presented {} of {} frames; square at [{:.0}, {:.0}]",
            self.presented,
            ctx.time().frame_count(),
            position[0],
            position[1]
        );
    }
}
