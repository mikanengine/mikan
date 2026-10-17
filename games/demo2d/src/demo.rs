//! The demo application: four scenes over the engine's real runtime.
//!
//! Everything gameplay-facing lives here. The demo implements [`Game`], so it
//! is driven by the engine's real runtime through the facade: the facade owns
//! the renderer and forwards resizes, `mikan-platform` owns the window, input,
//! and timing, and `mikan-render` draws.
//!
//! The ECS-driven scenes own a `mikan_scene::Scene`, which pairs a `World` with
//! a `Schedule` and the scene update order (systems, then deferred despawns,
//! then transform propagation). The classic scene's blocks and player are
//! entities with a `Transform2d` and a [`SpriteRenderer`]; each fixed step
//! moves the player's local transform and the scene propagates a
//! `GlobalTransform2d` that [`renderable`] turns into a sprite. The
//! `--scene hierarchy` scene goes further: its sun, planets, and moon are
//! linked by the transform hierarchy, so spinning the sun orbits its
//! descendants without per-body code.
//!
//! The classic player's movement decisions come from a Luau script loaded
//! through `mikan-script` (the `--script` option; ADR 0011). Assets are real
//! files loaded by `mikan-assets` at startup (see [`crate::assets`]); the
//! immediate-mode sprite layouts are in [`crate::scene`].
//!
//! [`SpriteRenderer`]: crate::renderable::SpriteRenderer

use std::time::Duration;

use mikan::{
    Camera2d, CaptureBuffer, DeadEntity, DeviceEvent, Engine, Entity, Frame, FrameOutcome,
    FrameStats, Game, KeyCode, PhysicalKey, RenderError, SamplerMode, Scene, ScriptEngine,
    ScriptError, Sprite, StepInput, Transform2d,
};
use thiserror::Error;

use crate::actions::ActionMapping;
use crate::assets::{AssetError, Assets};
use crate::blocks;
use crate::cli::{CameraMode, Cli, Scene as DemoScene};
use crate::hierarchy::{self, HierarchyScene, HierarchySetupError};
use crate::player::{Collider, Controls, Motion, Player, PreviousPosition, WorldBounds};
use crate::ppm;
use crate::renderable::{self, SpriteRenderer};
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

    /// Spawning or linking entities failed (a handle was not alive).
    #[error(transparent)]
    Ecs(#[from] DeadEntity),

    /// Building the hierarchy scene failed.
    #[error(transparent)]
    Hierarchy(#[from] HierarchySetupError),

    /// The gameplay script could not be loaded or validated.
    #[error(transparent)]
    Script(#[from] ScriptError),

    /// The action-mapping lane rejected an operation.
    #[error(transparent)]
    Input(#[from] mikan::InputError),

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

/// One step's already-resolved control intents, shared by the two input
/// lanes: the Luau script and the hierarchy spin read exactly these
/// five booleans whichever lane produced them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Intent {
    /// Move left this step.
    left: bool,
    /// Move right this step.
    right: bool,
    /// Move up this step.
    up: bool,
    /// Move down this step.
    down: bool,
    /// F fired this step (despawn the moon in the hierarchy scene).
    despawn_moon: bool,
}

/// Demo application state.
pub struct Demo {
    cli: Cli,
    /// Textures and sheet loaded at startup, kept alive for the sprites that
    /// borrow them.
    assets: Option<Assets>,
    /// The ECS-driven scenes' simulation: a world, its schedule, and the
    /// update order that propagates world transforms.
    scene: Scene,
    /// The classic scene's player entity, spawned in [`Game::init`].
    player: Option<Entity>,
    /// The hierarchy scene's bodies, spawned in [`Game::init`].
    hierarchy: Option<HierarchyScene>,
    /// The gameplay script driving the player, loaded in [`Game::init`].
    script: Option<ScriptEngine>,
    /// The `--actions` lane: action mapping over the native input module.
    /// `None` reads raw keys from [`mikan::InputState`] as before.
    actions: Option<ActionMapping>,
    /// Steps in which the wheel action carried a nonzero value.
    wheel_ticks: u32,
    /// A script step error has already been logged; later ones are suppressed
    /// so a broken script cannot flood the log at 120 Hz.
    script_error_reported: bool,
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
    /// Advances one fixed step from already-resolved intents: the movement
    /// booleans and whether F fired this step.
    fn step_with(
        &mut self,
        engine: &mut Engine<'_, '_>,
        dt: Duration,
        intent: Intent,
    ) -> Result<(), DemoError> {
        let Intent {
            left,
            right,
            up,
            down,
            despawn_moon,
        } = intent;
        // The step's external state, written before the systems run: the
        // controls the gameplay script asks for, the region the player is
        // kept in, and the hierarchy's spin. The script sees the held keys,
        // the step duration, and the player's position — never the world.
        let position = self
            .player
            .and_then(|entity| self.scene.world().get::<Transform2d>(entity))
            .map_or([0.0, 0.0], |transform| transform.translation);
        let controls = match self.script.as_ref() {
            Some(script) => match script.call_step(StepInput {
                dt: dt.as_secs_f32(),
                left,
                right,
                up,
                down,
                position,
            }) {
                Ok(output) => Controls {
                    direction: output.direction,
                    speed: output.speed,
                },
                // A gameplay bug must not kill the run: log the first error
                // with its script and line, then idle for the step.
                Err(error) => {
                    if !self.script_error_reported {
                        log::error!("script error: {error}");
                        self.script_error_reported = true;
                    }
                    Controls {
                        direction: [0.0, 0.0],
                        speed: 0.0,
                    }
                }
            },
            None => Controls {
                direction: [0.0, 0.0],
                speed: 0.0,
            },
        };
        let spin = match (left, right) {
            (false, true) => hierarchy::SPIN_SPEED,
            (true, false) => -hierarchy::SPIN_SPEED,
            _ => 0.0,
        };
        let (width, height) = engine.renderer().size();
        let view = self.world_view([width as f32, height as f32]);

        self.scene.world_mut().insert_resource(controls);
        self.scene.world_mut().insert_resource(WorldBounds(view));
        self.scene
            .world_mut()
            .insert_resource(hierarchy::Spin(spin));
        // The moon is removed through the scene's deferred queue, so the
        // despawn is applied at the next update's flush point rather than
        // mid-step.
        if despawn_moon && let Some(bodies) = self.hierarchy {
            log::info!("F pressed; despawning the moon");
            self.scene.despawn_later(bodies.moon());
        }

        let before = self
            .player
            .and_then(|entity| self.scene.world().get::<Transform2d>(entity))
            .map(|transform| transform.translation);
        self.scene.update(dt);
        self.fixed_steps += 1;
        let after = self
            .player
            .and_then(|entity| self.scene.world().get::<Transform2d>(entity))
            .map(|transform| transform.translation);
        if before != after {
            self.moved_steps += 1;
        }
        Ok(())
    }

    /// Builds the demo from parsed command-line options.
    #[must_use]
    pub fn new(cli: Cli) -> Self {
        let scene = Scene::new(cli.scene.name());
        Self {
            capture: cli.capture.as_ref().map(|_| CaptureBuffer::new()),
            cli,
            assets: None,
            scene,
            player: None,
            hierarchy: None,
            script: None,
            actions: None,
            wheel_ticks: 0,
            script_error_reported: false,
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
        let world_view = self.world_view(viewport);
        let mut sprites = Vec::new();
        match self.cli.scene {
            DemoScene::Classic => {
                // The blocks and the player are entities; the marker stays a
                // constant drawn over them.
                let override_position = self
                    .player
                    .map(|entity| (entity, self.player_pose(alpha).0));
                renderable::collect(
                    self.scene.world(),
                    assets,
                    sampler,
                    override_position,
                    &mut sprites,
                );
                sprites.push(scene::marker_sprite(assets, world_view, sampler));
            }
            DemoScene::Hierarchy => {
                renderable::collect(self.scene.world(), assets, sampler, None, &mut sprites);
            }
            DemoScene::Showcase => sprites = scene::showcase(assets, sampler),
            DemoScene::Stress => {
                if let Some(layout) = self.layout.as_ref() {
                    scene::stress_sprites(assets, layout, elapsed, sampler, &mut sprites);
                }
            }
        }
        sprites
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
            .and_then(|entity| crate::player::render_pose(self.scene.world(), entity, alpha))
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

    /// The number of gameplay components the ECS scenes carry, counted across
    /// the classic player and blocks.
    ///
    /// Deliberately narrow: the renderable and derived components
    /// ([`SpriteRenderer`], `GlobalTransform2d`) are not simulation state, so
    /// the startup summary counts only the five types the systems read and
    /// write.
    fn gameplay_components(&self) -> usize {
        let world = self.scene.world();
        world.component_count::<Transform2d>()
            + world.component_count::<PreviousPosition>()
            + world.component_count::<Motion>()
            + world.component_count::<Player>()
            + world.component_count::<Collider>()
    }

    /// Spawns the classic scene's entities: the blocks, then the player.
    fn spawn_classic(&mut self, view: [f32; 2], assets: &Assets) -> Result<(), DemoError> {
        let world = self.scene.world_mut();
        world.insert_resource(WorldBounds(view));
        world.insert_resource(Controls {
            direction: [0.0, 0.0],
            speed: 0.0,
        });
        // The blocks are spawned before the player: `renderable::collect`
        // draws entities in `SpriteRenderer` insertion order, so this keeps
        // the pillar under the player, as the pre-entity layout drew them.
        let _blocks = blocks::spawn_blocks(world)?;
        let player = crate::player::spawn_player(
            world,
            [view[0] * 0.5, view[1] * 0.5],
            scene::PLAYER_SIZE,
            scene::PLAYER_SPEED,
        )?;
        world.insert(
            player,
            SpriteRenderer::atlas([scene::PLAYER_SIZE; 2], assets.player_uv),
        )?;
        self.player = Some(player);
        *self.scene.schedule_mut() = crate::player::schedule();
        Ok(())
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
        if self.cli.actions {
            let mapping = ActionMapping::new()?;
            let contexts: Vec<String> = mapping
                .contexts()
                .iter()
                .map(|id| format!("{id}"))
                .collect();
            log::info!(
                "input: action mapping active (contexts {})",
                contexts.join(", "),
            );
            self.actions = Some(mapping);
        }

        // Start where the scene's default view is centered: the visible world
        // region's middle, so the player is on screen in every mode.
        let view = self.world_view([width as f32, height as f32]);

        match self.cli.scene {
            DemoScene::Classic => self.spawn_classic(view, &assets)?,
            DemoScene::Hierarchy => {
                let bodies = hierarchy::spawn(&mut self.scene, assets.player_uv, assets.marker_uv)?;
                self.hierarchy = Some(bodies);
            }
            DemoScene::Showcase | DemoScene::Stress => {}
        }
        // The first frame may render before the first fixed step runs, so the
        // scene's world transforms are propagated once here.
        self.scene.propagate();

        log::info!(
            "ecs: {} entities, {} components, systems: {}",
            self.scene.entity_count(),
            self.gameplay_components(),
            self.scene.systems().collect::<Vec<_>>().join(", "),
        );
        if self.cli.scene == DemoScene::Classic {
            log::info!(
                "collision: {} static blocks, grid cell {}",
                blocks::CLASSIC_BLOCKS.len(),
                blocks::GRID_CELL,
            );
        }
        if let Some(bodies) = self.hierarchy {
            let planets: Vec<Entity> = (0..hierarchy::PLANET_COUNT)
                .map(|index| bodies.planet(index))
                .collect();
            log::info!(
                "hierarchy: sun {} + {planets:?} + moon {} | orbit {}, moon orbit {} | A/D spin, F despawn",
                bodies.sun(),
                bodies.moon(),
                hierarchy::ORBIT_RADIUS,
                hierarchy::MOON_ORBIT_RADIUS,
            );
        }
        let script = ScriptEngine::from_file(&self.cli.script)?;
        log::info!("script: {} (mikan_step)", self.cli.script.display());
        self.script = Some(script);
        if self.cli.scene == DemoScene::Stress {
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
        // The engine runs two input lanes. With `--actions`, the step's
        // device events drain into the native pipeline and gameplay asks for
        // actions; the edges are step-scoped, cleared by `end_step` below.
        // Without it, the (frame-scoped, read-only) InputState snapshot
        // answers the same questions, which is what the verify scripts drive.
        let intent = if let Some(mapping) = self.actions.as_mut() {
            let mut events: Vec<DeviceEvent> = Vec::new();
            engine.ctx().input_mut().take_events(&mut events);
            mapping.process(&events)?;
            let movement = mapping.movement();
            let despawn = mapping.despawn_pressed();
            let quit = mapping.quit_pressed();
            let scroll = mapping.scroll_y();
            mapping.end_step();
            if quit {
                log::info!("escape pressed; quitting");
                engine.ctx().request_exit();
                return Ok(());
            }
            self.wheel_ticks += u32::from(scroll != 0.0);
            Some((movement, despawn))
        } else {
            None
        };
        if let Some((movement, despawn_moon)) = intent {
            let (left, right, up, down) = movement;
            self.step_with(
                engine,
                dt,
                Intent {
                    left,
                    right,
                    up,
                    down,
                    despawn_moon,
                },
            )?;
            return Ok(());
        }
        // The raw-key lane: the borrow below ends inside this block, before
        // `step_with` borrows the engine again. Escape comes first, exactly
        // as it did before the action lane existed.
        {
            let escape_pressed = engine
                .ctx()
                .input()
                .is_key_just_pressed(PhysicalKey::Code(KeyCode::Escape));
            if escape_pressed {
                log::info!("escape pressed; quitting");
                engine.ctx().request_exit();
                return Ok(());
            }
            let input = engine.ctx().input();
            let held = |key: KeyCode| input.is_key_pressed(PhysicalKey::Code(key));
            let intent = Intent {
                left: held(KeyCode::KeyA) || held(KeyCode::ArrowLeft),
                right: held(KeyCode::KeyD) || held(KeyCode::ArrowRight),
                up: held(KeyCode::KeyW) || held(KeyCode::ArrowUp),
                down: held(KeyCode::KeyS) || held(KeyCode::ArrowDown),
                despawn_moon: input.is_key_just_pressed(PhysicalKey::Code(KeyCode::KeyF)),
            };
            self.step_with(engine, dt, intent)
        }
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
            .and_then(|entity| self.scene.world().get::<Transform2d>(entity))
            .map_or([0.0, 0.0], |transform| transform.translation);
        log::info!(
            "demo finished: presented {} of {} frames; square at [{:.0}, {:.0}]",
            self.presented,
            engine.ctx().time().frame_count(),
            position[0],
            position[1]
        );
        log::info!(
            "ecs: {} entities, {} fixed steps, {} moved steps",
            self.scene.entity_count(),
            self.fixed_steps,
            self.moved_steps,
        );
        if self.actions.is_some() {
            log::info!(
                "actions: {} fixed steps, {} moved, {} wheel ticks",
                self.fixed_steps,
                self.moved_steps,
                self.wheel_ticks,
            );
        }
    }
}
