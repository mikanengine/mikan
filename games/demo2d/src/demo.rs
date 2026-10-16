//! The demo application: three scenes over the engine's real runtime.
//!
//! Everything gameplay-facing lives here. The demo implements [`Game`], so it
//! is driven by the engine's real runtime through the facade: the facade owns
//! the renderer and forwards resizes, `mikan-platform` owns the window, input,
//! and timing, and `mikan-render` draws. The classic scene's player is a
//! `mikan-ecs` entity whose systems run once per fixed step (see
//! [`crate::player`]).
//!
//! Assets are real files loaded by `mikan-assets` at startup (see
//! [`crate::assets`]); the sprite layouts are in [`crate::scene`].

use std::time::Duration;

use mikan::{
    AppContext, Camera2d, CaptureBuffer, DeadEntity, Engine, Entity, Frame, FrameOutcome,
    FrameStats, Game, KeyCode, PhysicalKey, RenderError, SamplerMode, Schedule, Sprite, World,
};
use thiserror::Error;

use crate::assets::{AssetError, Assets};
use crate::cli::{CameraMode, Cli, Scene};
use crate::player::{
    Motion, MoveDirection, Player, Position, PreviousPosition, WorldBounds, direction_from_keys,
};
use crate::ppm;
use crate::scene::{self, StressLayout};

/// Failures the demo itself can report.
#[derive(Debug, Error)]
pub enum DemoError {
    /// Rendering failed.
    #[error(transparent)]
    Render(#[from] RenderError),

    /// Loading the demo's assets failed.
    #[error(transparent)]
    Asset(#[from] AssetError),

    /// Writing a captured frame failed.
    #[error(transparent)]
    Image(#[from] ppm::PpmError),

    /// Spawning the player entity failed (a handle was not alive).
    #[error(transparent)]
    Ecs(#[from] DeadEntity),

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
    /// Textures and sheet loaded at startup, kept alive for the sprites that
    /// borrow them.
    assets: Option<Assets>,
    /// The classic scene's simulation: one player entity and its schedule.
    world: World,
    schedule: Schedule,
    /// The player entity, spawned in [`Game::init`].
    player: Option<Entity>,
    /// Fixed steps run, and how many of them changed the player's position.
    /// Diagnostics for the shutdown log and the runtime verification.
    fixed_steps: u64,
    moved_steps: u64,
    /// Stress-scene grid, built once the viewport is known.
    layout: Option<StressLayout>,
    presented: u64,
    capture: Option<CaptureBuffer>,
    captured: bool,
    capture_finalized: bool,
    frame_times: Vec<Duration>,
    idle_warned: bool,
    /// Statistics of the last frame submitted to the GPU.
    last_stats: FrameStats,
    /// Highest batch count seen, for reporting that batching held up.
    max_batches: usize,
}

impl Demo {
    /// Builds the demo from parsed command-line options.
    #[must_use]
    pub fn new(cli: Cli) -> Self {
        Self {
            capture: cli.capture.as_ref().map(|_| CaptureBuffer::new()),
            cli,
            assets: None,
            world: World::new(),
            schedule: crate::player::schedule(),
            player: None,
            fixed_steps: 0,
            moved_steps: 0,
            layout: None,
            presented: 0,
            captured: false,
            capture_finalized: false,
            frame_times: Vec::new(),
            idle_warned: false,
            last_stats: FrameStats::default(),
            max_batches: 0,
        }
    }

    /// The sampler the scenes use unless they pin their own.
    fn sampler(&self) -> SamplerMode {
        self.cli.sampler.unwrap_or_default()
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

    /// The world-space size of the visible region: what the camera shows.
    ///
    /// One world unit is one physical pixel without a camera; a zoom shows
    /// `viewport / zoom` world units, and pixel-perfect mode shows exactly the
    /// virtual screen.
    fn world_view(&self, viewport: [f32; 2]) -> [f32; 2] {
        if let Some(virtual_size) = self.cli.pixel_perfect {
            return [virtual_size[0] as f32, virtual_size[1] as f32];
        }
        match self.cli.camera {
            CameraMode::None => viewport,
            CameraMode::Follow => [viewport[0] / self.cli.zoom, viewport[1] / self.cli.zoom],
        }
    }

    /// Sprites for this frame, in painter order.
    ///
    /// `assets` is a parameter rather than a field read so the returned
    /// sprites borrow only the assets, which keeps the mutable capture borrow
    /// free at the call site.
    fn sprites<'a>(
        &self,
        assets: &'a Assets,
        viewport: [f32; 2],
        elapsed: f32,
        alpha: f32,
    ) -> Vec<Sprite<'a>> {
        let sampler = self.sampler();
        let world = self.world_view(viewport);
        match self.cli.scene {
            Scene::Classic => {
                let (center, size) = self.player_pose(alpha);
                scene::classic(assets, center, size, world, sampler).to_vec()
            }
            Scene::Showcase => scene::showcase(assets, sampler),
            Scene::Stress => {
                let mut sprites = Vec::new();
                if let Some(layout) = self.layout.as_ref() {
                    scene::stress_sprites(assets, layout, elapsed, sampler, &mut sprites);
                }
                sprites
            }
        }
    }

    /// Camera for this frame, or `None` for the pixel-space view.
    fn camera(&self, viewport: [u32; 2], alpha: f32) -> Option<Camera2d> {
        if let Some(virtual_size) = self.cli.pixel_perfect {
            let position = match self.cli.camera {
                // Without a following camera the virtual screen sits at the
                // world origin, like a fixed game screen.
                CameraMode::None => [virtual_size[0] as f32 * 0.5, virtual_size[1] as f32 * 0.5],
                CameraMode::Follow => self.player_pose(alpha).0,
            };
            return Some(Camera2d::pixel_perfect(position, viewport, virtual_size));
        }
        match self.cli.camera {
            CameraMode::None => None,
            // The camera centers on the interpolated player position, so the
            // player stays still on screen while the world scrolls.
            CameraMode::Follow => Some(Camera2d::new(self.player_pose(alpha).0, self.cli.zoom)),
        }
    }

    /// The player's interpolated render pose for this frame.
    ///
    /// Falls back to a degenerate square before [`Game::init`] has spawned the
    /// entity, so a frame that slips through before initialization draws
    /// nothing visible instead of panicking.
    fn player_pose(&self, alpha: f32) -> ([f32; 2], f32) {
        self.player
            .and_then(|entity| crate::player::render_pose(&self.world, entity, alpha))
            .unwrap_or(([0.0, 0.0], 0.0))
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

    /// Prints frame-time and draw statistics collected during the run.
    fn print_stats(&self) {
        if !self.cli.stats {
            return;
        }
        let sampler = sampler_name(self.sampler());
        println!(
            "render: scene {} | sampler {} | sprites {} | batches {} | draw calls {} \
             (last presented frame, max batches {})",
            self.cli.scene.name(),
            sampler,
            self.last_stats.sprites,
            self.last_stats.batches,
            self.last_stats.draw_calls,
            self.max_batches,
        );

        if self.frame_times.is_empty() {
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

/// Display name of a sampler mode, for logs and statistics.
fn sampler_name(sampler: SamplerMode) -> &'static str {
    match sampler {
        SamplerMode::Linear => "linear",
        SamplerMode::Nearest => "nearest",
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

        let assets = Assets::load(engine.renderer(), &self.cli.assets)?;
        let info = engine.renderer().adapter_info();
        log::info!(
            "rendering with {} ({:?}), swap chain format {:?}",
            info.name,
            info.backend,
            engine.renderer().surface_format()
        );
        let (width, height) = engine.renderer().size();
        let (atlas_width, atlas_height) = assets.atlas.size();
        let (star_width, star_height) = assets.star.size();
        log::info!(
            "assets: {} -> atlas {}x{} ({} frames), star {}x{}",
            self.cli.assets.display(),
            atlas_width,
            atlas_height,
            assets.sheet.len(),
            star_width,
            star_height,
        );
        log::info!(
            "scene {} | sampler {} | camera {:?} | vsync {} | window {}x{}",
            self.cli.scene.name(),
            sampler_name(self.sampler()),
            self.cli.camera,
            self.cli.vsync,
            width,
            height,
        );
        if let Some(virtual_size) = self.cli.pixel_perfect {
            log::info!(
                "pixel-perfect: virtual {}x{}, integer zoom x{}",
                virtual_size[0],
                virtual_size[1],
                Camera2d::pixel_perfect([0.0, 0.0], [width, height], virtual_size).effective_zoom(),
            );
        }
        log::info!("controls: WASD or arrow keys to move, Escape to quit");

        // Start where the scene's default view is centered: the visible world
        // region's middle, so the player is on screen in every mode.
        let view = self.world_view([width as f32, height as f32]);
        self.world.insert_resource(WorldBounds(view));
        self.world.insert_resource(MoveDirection([0.0, 0.0]));
        let player = crate::player::spawn_player(
            &mut self.world,
            [view[0] * 0.5, view[1] * 0.5],
            scene::PLAYER_SIZE,
            scene::PLAYER_SPEED,
        )?;
        self.player = Some(player);
        log::info!(
            "ecs: {} entity, {} components, systems: {}",
            self.world.entity_count(),
            self.world.component_count::<Position>()
                + self.world.component_count::<PreviousPosition>()
                + self.world.component_count::<Motion>()
                + self.world.component_count::<Player>(),
            self.schedule.names().collect::<Vec<_>>().join(", "),
        );
        if self.cli.scene == Scene::Stress {
            let layout =
                StressLayout::new(self.cli.stress_sprites(), [width as f32, height as f32]);
            log::info!(
                "stress: {} sprites, {}x{} cells",
                layout.len(),
                layout.cell()[0].round(),
                layout.cell()[1].round(),
            );
            self.layout = Some(layout);
        }
        self.assets = Some(assets);
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
        let view = self.world_view([width as f32, height as f32]);

        // The step's external state, written before the systems run: the
        // direction the keys ask for and the region the player is kept in.
        self.world.insert_resource(MoveDirection(direction));
        self.world.insert_resource(WorldBounds(view));

        let before = self
            .player
            .and_then(|entity| self.world.get::<Position>(entity))
            .map(|position| position.0);
        self.schedule.run(&mut self.world, dt);
        self.fixed_steps += 1;
        let after = self
            .player
            .and_then(|entity| self.world.get::<Position>(entity))
            .map(|position| position.0);
        if before != after {
            self.moved_steps += 1;
        }
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
        let elapsed = engine.ctx().time().elapsed().as_secs_f32();
        let clear_color = engine.ctx().config().clear_color;
        let (width, height) = engine.renderer().size();

        // Sprites borrow the assets; the borrow ends after the render call,
        // which is when the capture buffer is borrowed mutably.
        let Some(assets) = self.assets.as_ref() else {
            return Ok(FrameOutcome::Idle);
        };
        let sprites = self.sprites(assets, [width as f32, height as f32], elapsed, alpha);
        let camera = self.camera([width, height], alpha);
        let mut frame = Frame::new(clear_color).with_sprites(&sprites);
        if let Some(camera) = camera {
            frame = frame.with_camera(camera);
        }

        let capturing = self.capture.is_some();
        let outcome = engine.render_frame(&frame, self.capture.as_mut())?;
        if capturing {
            self.captured = true;
        }
        self.last_stats = engine.renderer().last_frame_stats();
        self.max_batches = self.max_batches.max(self.last_stats.batches);

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
        let position = self
            .player
            .and_then(|entity| self.world.get::<Position>(entity))
            .map_or([0.0, 0.0], |position| position.0);
        log::info!(
            "demo finished: presented {} of {} frames; square at [{:.0}, {:.0}]",
            self.presented,
            engine.ctx().time().frame_count(),
            position[0],
            position[1]
        );
        log::info!(
            "ecs: {} entity, {} fixed steps, {} moved steps",
            self.world.entity_count(),
            self.fixed_steps,
            self.moved_steps,
        );
    }
}
