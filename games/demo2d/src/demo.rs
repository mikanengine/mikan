//! The demo application: a square you can drive around with the keyboard.
//!
//! Everything gameplay-facing lives here. The demo implements [`Game`], so it
//! is driven by the engine's real runtime through the facade: the facade owns
//! the renderer and forwards resizes, `mikan-platform` owns the window, input,
//! and timing, and `mikan-render` draws.
//!
//! Textures come from a procedural atlas (see [`crate::atlas`]); nothing is
//! loaded from disk yet.

use std::time::Duration;

use mikan::{
    AppContext, Camera2d, CaptureBuffer, Engine, Frame, FrameOutcome, Game, KeyCode, PhysicalKey,
    RenderError, Sprite, Texture, TextureDescriptor,
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
    Render(#[from] RenderError),

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
    /// Atlas texture uploaded at startup, kept alive for the sprites that
    /// borrow it.
    atlas: Option<Texture>,
    square: Square,
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
            atlas: None,
            square: Square::new([0.0, 0.0], SQUARE_SIZE, SQUARE_SPEED),
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
    /// The returned sprites borrow `atlas`, which is a parameter rather than a
    /// field read so the borrow stays visible at the call site.
    fn sprites<'t>(&self, atlas: &'t Texture, viewport: [f32; 2], alpha: f32) -> [Sprite<'t>; 2] {
        let center = self.square.interpolated_position(alpha);
        let size = self.square.size();
        let marker_center = [
            MARKER_MARGIN + MARKER_SIZE * 0.5,
            (viewport[1] - MARKER_MARGIN - MARKER_SIZE * 0.5).max(MARKER_SIZE * 0.5),
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

impl Game for Demo {
    type Error = DemoError;

    fn init(&mut self, engine: &mut Engine<'_, '_>) -> Result<(), DemoError> {
        // Capture was requested on the command line; fail before the first
        // frame if this surface cannot provide it.
        if self.cli.capture.is_some() && !engine.renderer().supports_capture() {
            return Err(DemoError::CaptureUnsupported);
        }

        let atlas_data = atlas::build();
        let atlas = engine.renderer().create_texture(TextureDescriptor::new(
            atlas_data.width,
            atlas_data.height,
            &atlas_data.pixels,
        ))?;

        let info = engine.renderer().adapter_info();
        log::info!(
            "rendering with {} ({:?}), swap chain format {:?}",
            info.name,
            info.backend,
            engine.renderer().surface_format()
        );
        log::info!(
            "textures: procedural atlas {}x{}, {} tiles of {}px",
            atlas_data.width,
            atlas_data.height,
            atlas::ATLAS_COLUMNS * atlas::ATLAS_ROWS,
            atlas::TILE_SIZE
        );
        log::info!("controls: WASD or arrow keys to move, Escape to quit");

        let (width, height) = engine.renderer().size();
        self.square = Square::new(
            [width as f32 * 0.5, height as f32 * 0.5],
            SQUARE_SIZE,
            SQUARE_SPEED,
        );
        self.atlas = Some(atlas);
        Ok(())
    }

    fn update(&mut self, engine: &mut Engine<'_, '_>, dt: Duration) -> Result<(), DemoError> {
        if engine
            .ctx()
            .input()
            .is_key_just_pressed(PhysicalKey::Code(KeyCode::Escape))
        {
            log::info!("escape pressed; quitting");
            engine.ctx().request_exit();
            return Ok(());
        }

        let direction = Self::direction(engine.ctx());
        let (width, height) = engine.renderer().size();
        self.square
            .fixed_update(direction, dt, [width.max(1) as f32, height.max(1) as f32]);
        Ok(())
    }

    fn render(&mut self, engine: &mut Engine<'_, '_>) -> Result<FrameOutcome, DemoError> {
        if let Some(timeout) = self.cli.timeout
            && let Some(target) = self.cli.frames
            && self.presented < target
            && engine.ctx().time().elapsed() > timeout
        {
            return Err(DemoError::FrameTimeout {
                presented: self.presented,
                requested: target,
                timeout_secs: timeout.as_secs(),
            });
        }

        let alpha = engine.ctx().alpha();
        let clear_color = engine.ctx().config().clear_color;
        let (width, height) = engine.renderer().size();

        // Sprites borrow the atlas; the borrow ends after the render call,
        // which is when the capture buffer is borrowed mutably.
        let Some(atlas) = self.atlas.as_ref() else {
            return Ok(FrameOutcome::Idle);
        };
        let sprites = self.sprites(atlas, [width as f32, height as f32], alpha);
        let camera = self.camera(alpha);
        let mut frame = Frame::new(clear_color).with_sprites(&sprites);
        if let Some(camera) = camera {
            frame = frame.with_camera(camera);
        }

        let capturing = self.capture.is_some();
        let outcome = engine.render_frame(&frame, self.capture.as_mut())?;
        if capturing {
            self.captured = true;
        }

        match outcome {
            FrameOutcome::Presented => {
                self.presented += 1;
                if self.cli.stats {
                    self.frame_times.push(engine.ctx().time().delta());
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
            self.write_capture()?;
            engine.ctx().request_exit();
        }

        Ok(outcome)
    }

    fn shutdown(&mut self, engine: &mut Engine<'_, '_>) {
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
            engine.ctx().time().frame_count(),
            position[0],
            position[1]
        );
    }
}
