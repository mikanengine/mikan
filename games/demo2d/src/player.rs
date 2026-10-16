//! The classic scene's player: components, resources, and systems.
//!
//! The player is an ordinary entity. Its schedule runs five named systems,
//! once per fixed step, in exactly this order:
//!
//! 1. `snapshot` — copies [`Position`] into [`PreviousPosition`] so rendering
//!    can interpolate between the last two fixed states.
//! 2. `input` — copies this step's [`Controls`] into [`Motion`] (the controls
//!    come from the gameplay script; see `mikan-script` and ADR 0011).
//! 3. `movement` — integrates [`Motion`] into [`Position`].
//! 4. `collide` — resolves [`Collider`] overlaps against the static blocks
//!    (the system lives in [`crate::blocks`]).
//! 5. `clamp` — keeps the player inside [`WorldBounds`].
//!
//! The order is the schedule: it is stated once, in [`schedule`], and the list
//! above documents it. Step 1 must precede step 3 (or interpolation would
//! blend the current position with itself), step 4 must follow step 3 (or the
//! player could end a step inside a block), and step 5 must follow step 4 (or
//! a resolution could push the player out of the world).
//!
//! The external state of a step — the controls the script asks for and the
//! world bounds — travels through resources. The game writes them before the
//! schedule runs; the systems read them and know nothing about the window,
//! the input API, the script VM, or the renderer, which is what keeps them
//! unit-testable.

use mikan::{DeadEntity, Entity, Schedule, World};

/// World-space center of an entity, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position(pub [f32; 2]);

/// [`Position`] as of the previous fixed step.
///
/// Rendering interpolates between this and [`Position`] with the platform's
/// interpolation alpha, so a 120 Hz simulation still looks smooth on another
/// refresh rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviousPosition(pub [f32; 2]);

/// Movement request: a unit direction and a speed in pixels per second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    /// Unit-length direction for this step; zero means "stay".
    pub direction: [f32; 2],
    /// Speed in pixels per second, applied as `speed * dt`.
    pub speed: f32,
}

/// Marker component: this entity is the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Player {
    /// Side length of the player square, in pixels.
    pub size: f32,
}

/// The box an entity occupies for collision: centered on its [`Position`],
/// `size` pixels wide and tall.
///
/// Every entity with a [`Position`] and a [`Collider`] takes part in the
/// `collide` system ([`crate::blocks::collide_system`]). Entities that also
/// carry [`Motion`] absorb the resolution, so the player is pushed clear of
/// the blocks while the blocks themselves never move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Collider {
    /// Full width and height in pixels.
    pub size: [f32; 2],
}

/// The controls the player is asked to use this step (a resource).
///
/// The demo writes this from the Luau gameplay script's decision every fixed
/// step: the script owns intent, the systems own simulation (ADR 0011).
/// `input` copies it into [`Motion`]; a missing resource makes the player
/// idle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Controls {
    /// Direction of at most unit length; zero means "stay".
    pub direction: [f32; 2],
    /// Speed in pixels per second, applied as `speed * dt`.
    pub speed: f32,
}

/// The size of the world region the player is kept inside (a resource).
///
/// This is the visible world area, so it follows the camera mode: the whole
/// viewport without a camera, or the zoomed/pixel-perfect region with one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldBounds(pub [f32; 2]);

/// Spawns the player entity with every component it needs.
///
/// # Errors
///
/// Returns [`DeadEntity`] if the freshly spawned entity is somehow not alive;
/// by construction this cannot happen, but the signature keeps the caller from
/// unwrapping a failure path.
pub fn spawn_player(
    world: &mut World,
    position: [f32; 2],
    size: f32,
    speed: f32,
) -> Result<Entity, DeadEntity> {
    let entity = world.spawn();
    world.insert(entity, Position(position))?;
    world.insert(entity, PreviousPosition(position))?;
    world.insert(
        entity,
        Motion {
            direction: [0.0, 0.0],
            speed,
        },
    )?;
    world.insert(entity, Player { size })?;
    world.insert(entity, Collider { size: [size, size] })?;
    Ok(entity)
}

/// Builds the player's schedule: snapshot, input, movement, collide, clamp.
#[must_use]
pub fn schedule() -> Schedule {
    let mut schedule = Schedule::new();
    schedule
        .add("snapshot", |world, _dt| {
            for (_, previous, position) in world.iter2_mut::<PreviousPosition, Position>() {
                previous.0 = position.0;
            }
        })
        .add("input", |world, _dt| {
            let controls = world.get_resource::<Controls>().copied();
            for (_, motion) in world.iter_mut::<Motion>() {
                match controls {
                    Some(controls) => {
                        motion.direction = controls.direction;
                        motion.speed = controls.speed;
                    }
                    None => motion.direction = [0.0, 0.0],
                }
            }
        })
        .add("movement", |world, dt| {
            // The exact per-step arithmetic of the original `Square`, kept
            // bit-for-bit so the milestone that introduced the ECS did not
            // move the player: speed * dt first, then direction * distance.
            for (_, position, motion) in world.iter2_mut::<Position, Motion>() {
                let distance = motion.speed * dt.as_secs_f32();
                position.0[0] += motion.direction[0] * distance;
                position.0[1] += motion.direction[1] * distance;
            }
        })
        .add("collide", crate::blocks::collide_system())
        .add("clamp", |world, _dt| {
            // Copy the bounds out first: holding a resource borrow across the
            // iteration below would borrow the world twice.
            let Some(bounds) = world.get_resource::<WorldBounds>().map(|bounds| bounds.0) else {
                return;
            };
            for (_, position, player) in world.iter2_mut::<Position, Player>() {
                let half = player.size * 0.5;
                position.0[0] = clamp_center(position.0[0], half, bounds[0]);
                position.0[1] = clamp_center(position.0[1], half, bounds[1]);
            }
        });
    schedule
}

/// The player's render pose for this frame: interpolated center and square
/// size, or `None` if the entity (or one of its components) is missing.
#[must_use]
pub fn render_pose(world: &World, entity: Entity, alpha: f32) -> Option<([f32; 2], f32)> {
    let position = world.get::<Position>(entity)?.0;
    let previous = world.get::<PreviousPosition>(entity)?.0;
    let size = world.get::<Player>(entity)?.size;
    Some((interpolated_position(previous, position, alpha), size))
}

/// Position to draw after `alpha` of the current fixed step has elapsed.
///
/// Interpolating between the previous and current simulation states keeps
/// motion smooth when the display refresh rate is not a multiple of the fixed
/// update rate. This is pure: rendering never mutates the simulation.
#[must_use]
pub fn interpolated_position(previous: [f32; 2], position: [f32; 2], alpha: f32) -> [f32; 2] {
    let alpha = alpha.clamp(0.0, 1.0);
    [
        previous[0] + (position[0] - previous[0]) * alpha,
        previous[1] + (position[1] - previous[1]) * alpha,
    ]
}

/// Keeps a center coordinate inside a world that is `extent` pixels wide.
///
/// A world smaller than the object collapses the object to the center
/// instead of panicking like `f32::clamp` would.
fn clamp_center(value: f32, half: f32, extent: f32) -> f32 {
    if extent <= half * 2.0 {
        extent * 0.5
    } else {
        value.clamp(half, extent - half)
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use super::*;

    const VIEWPORT: [f32; 2] = [800.0, 600.0];

    fn millis(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    /// The standard test world: one player, the resources the systems read,
    /// and the real schedule.
    fn fixture(position: [f32; 2], size: f32, speed: f32) -> (World, Entity, Schedule) {
        let mut world = World::new();
        world.insert_resource(WorldBounds(VIEWPORT));
        world.insert_resource(Controls {
            direction: [0.0, 0.0],
            speed: 0.0,
        });
        let player =
            spawn_player(&mut world, position, size, speed).expect("a fresh entity is alive");
        (world, player, schedule())
    }

    /// Writes the controls resource and runs one fixed step at `speed`.
    fn step(
        world: &mut World,
        schedule: &mut Schedule,
        direction: [f32; 2],
        speed: f32,
        dt: Duration,
    ) {
        world.insert_resource(Controls { direction, speed });
        schedule.run(world, dt);
    }

    #[test]
    fn movement_uses_delta_time() {
        let (mut world, player, mut schedule) = fixture([400.0, 300.0], 64.0, 100.0);
        step(&mut world, &mut schedule, [1.0, 0.0], 100.0, millis(500));
        assert_eq!(
            world.get::<Position>(player).unwrap().0,
            [450.0, 300.0],
            "100 px/s * 0.5 s"
        );

        step(&mut world, &mut schedule, [1.0, 0.0], 100.0, millis(250));
        assert_eq!(
            world.get::<Position>(player).unwrap().0,
            [475.0, 300.0],
            "delta time keeps scaling"
        );
    }

    #[test]
    fn snapshot_runs_before_movement() {
        let (mut world, player, mut schedule) = fixture([400.0, 300.0], 64.0, 100.0);
        step(&mut world, &mut schedule, [1.0, 0.0], 100.0, millis(500));
        assert_eq!(
            world.get::<PreviousPosition>(player).unwrap().0,
            [400.0, 300.0],
            "the previous position is the state before this step's move"
        );
        assert_eq!(world.get::<Position>(player).unwrap().0, [450.0, 300.0]);
    }

    #[test]
    fn a_player_without_input_does_not_move_at_all() {
        let (mut world, player, mut schedule) = fixture([400.0, 300.0], 64.0, 100.0);
        step(&mut world, &mut schedule, [0.0, 0.0], 100.0, millis(500));
        assert_eq!(world.get::<Position>(player).unwrap().0, [400.0, 300.0]);
        assert_eq!(
            world.get::<PreviousPosition>(player).unwrap().0,
            [400.0, 300.0]
        );
    }

    #[test]
    fn diagonal_movement_is_not_faster() {
        // The script hands the engine a unit diagonal (the engine's own
        // normalization is tested in mikan-script); the movement system must
        // apply `speed * dt` along it like any other direction.
        let direction = [core::f32::consts::FRAC_1_SQRT_2; 2];
        let (mut world, player, mut schedule) = fixture([400.0, 300.0], 64.0, 100.0);
        step(&mut world, &mut schedule, direction, 100.0, millis(1000));
        let moved = &world.get::<Position>(player).unwrap().0;
        let distance = ((moved[0] - 400.0).powi(2) + (moved[1] - 300.0).powi(2)).sqrt();
        assert!((distance - 100.0).abs() < 1e-3, "moved {distance} px");
    }

    #[test]
    fn the_controls_speed_applies_instead_of_the_spawn_speed() {
        let (mut world, player, mut schedule) = fixture([400.0, 300.0], 64.0, 100.0);
        step(&mut world, &mut schedule, [1.0, 0.0], 250.0, millis(1000));
        assert_eq!(
            world.get::<Position>(player).unwrap().0,
            [650.0, 300.0],
            "250 px/s comes from the controls, not the 100 px/s spawn value"
        );
    }

    #[test]
    fn a_missing_controls_resource_stops_the_player() {
        let (mut world, player, mut schedule) = fixture([400.0, 300.0], 64.0, 100.0);
        step(&mut world, &mut schedule, [1.0, 0.0], 100.0, millis(100));
        let moved = world.get::<Position>(player).unwrap().0;
        assert_ne!(moved, [400.0, 300.0], "it moved while controlled");

        world.remove_resource::<Controls>();
        schedule.run(&mut world, millis(1000));
        assert_eq!(
            world.get::<Position>(player).unwrap().0,
            moved,
            "without the resource the input system zeroes the direction"
        );
    }

    #[test]
    fn the_player_stays_inside_the_world_bounds() {
        let (mut world, player, mut schedule) = fixture([50.0, 50.0], 64.0, 1000.0);
        // Walk far past every edge; `clamp` runs after `movement` each step.
        step(
            &mut world,
            &mut schedule,
            [-1.0, -1.0],
            1000.0,
            millis(1000),
        );
        assert_eq!(
            world.get::<Position>(player).unwrap().0,
            [32.0, 32.0],
            "clamped to the top-left"
        );

        step(&mut world, &mut schedule, [1.0, 1.0], 1000.0, millis(1000));
        assert_eq!(
            world.get::<Position>(player).unwrap().0,
            [768.0, 568.0],
            "clamped to the bottom-right"
        );
    }

    #[test]
    fn a_world_smaller_than_the_player_does_not_panic() {
        let (mut world, player, mut schedule) = fixture([10.0, 10.0], 64.0, 100.0);
        world.insert_resource(WorldBounds([32.0, 32.0]));
        step(&mut world, &mut schedule, [1.0, 1.0], 100.0, millis(100));
        assert_eq!(world.get::<Position>(player).unwrap().0, [16.0, 16.0]);
    }

    #[test]
    fn a_missing_bounds_resource_disables_clamping_only() {
        let (mut world, player, mut schedule) = fixture([50.0, 50.0], 64.0, 1000.0);
        world.remove_resource::<WorldBounds>();
        step(&mut world, &mut schedule, [-1.0, -1.0], 1000.0, millis(100));
        assert_eq!(
            world.get::<Position>(player).unwrap().0,
            [-50.0, -50.0],
            "movement still ran; only the clamp was skipped"
        );
    }

    #[test]
    fn interpolation_blends_previous_and_current_positions() {
        assert_eq!(
            interpolated_position([400.0, 300.0], [410.0, 300.0], 0.0)[0],
            400.0
        );
        assert_eq!(
            interpolated_position([400.0, 300.0], [410.0, 300.0], 1.0)[0],
            410.0
        );
        assert_eq!(
            interpolated_position([400.0, 300.0], [410.0, 300.0], 0.5)[0],
            405.0
        );
        // Out-of-range alphas are clamped rather than extrapolated.
        assert_eq!(
            interpolated_position([400.0, 300.0], [410.0, 300.0], 2.0)[0],
            410.0
        );
        assert_eq!(
            interpolated_position([400.0, 300.0], [410.0, 300.0], -1.0)[0],
            400.0
        );
    }

    #[test]
    fn rendering_never_moves_the_simulation() {
        let (mut world, player, mut schedule) = fixture([400.0, 300.0], 64.0, 100.0);
        step(&mut world, &mut schedule, [1.0, 0.0], 100.0, millis(100));
        let before = world.get::<Position>(player).unwrap().0;
        for alpha in [0.0, 0.25, 0.5, 1.0] {
            let _ = render_pose(&world, player, alpha);
        }
        assert_eq!(world.get::<Position>(player).unwrap().0, before);
        assert_eq!(
            world.get::<PreviousPosition>(player).unwrap().0,
            [400.0, 300.0],
            "interpolation does not touch the stored states either"
        );
    }

    #[test]
    fn render_pose_interpolates_between_the_two_states() {
        let (mut world, player, mut schedule) = fixture([400.0, 300.0], 64.0, 100.0);
        step(&mut world, &mut schedule, [1.0, 0.0], 100.0, millis(1000));
        let (center, size) = render_pose(&world, player, 0.0).unwrap();
        assert_eq!((center, size), ([400.0, 300.0], 64.0), "alpha 0: previous");
        let (center, size) = render_pose(&world, player, 1.0).unwrap();
        assert_eq!((center, size), ([500.0, 300.0], 64.0), "alpha 1: current");
    }

    #[test]
    fn spawn_player_attaches_every_component() {
        let mut world = World::new();
        let player = spawn_player(&mut world, [1.0, 2.0], 96.0, 420.0).unwrap();
        assert!(world.has::<Position>(player));
        assert!(world.has::<PreviousPosition>(player));
        assert!(world.has::<Motion>(player));
        assert!(world.has::<Player>(player));
        assert!(world.has::<Collider>(player));
        assert_eq!(world.entity_count(), 1);
        assert_eq!(world.component_count::<Position>(), 1);
    }

    #[test]
    fn entities_without_the_player_components_are_ignored() {
        let (mut world, player, mut schedule) = fixture([400.0, 300.0], 64.0, 100.0);
        let bystander = world.spawn();
        world.insert(bystander, Position([1.0, 1.0])).unwrap();
        step(&mut world, &mut schedule, [1.0, 0.0], 100.0, millis(1000));

        assert_eq!(
            world.get::<Position>(player).unwrap().0,
            [500.0, 300.0],
            "the player moved"
        );
        assert_eq!(
            world.get::<Position>(bystander).unwrap().0,
            [1.0, 1.0],
            "an entity without a Motion never moves"
        );
    }

    #[test]
    fn the_schedule_names_state_the_execution_order() {
        let schedule = schedule();
        assert_eq!(
            schedule.names().collect::<Vec<_>>(),
            ["snapshot", "input", "movement", "collide", "clamp"]
        );
    }

    #[test]
    fn identical_step_sequences_reproduce_positions() {
        let steps: Vec<([f32; 2], Duration)> = (0..50)
            .map(|index| {
                let direction = if index % 3 == 0 {
                    [1.0, 0.5]
                } else {
                    [-0.25, 1.0]
                };
                (direction, millis(8 + index % 5))
            })
            .collect();

        let run = || {
            let (mut world, player, mut schedule) = fixture([400.0, 300.0], 64.0, 420.0);
            for (direction, dt) in &steps {
                step(&mut world, &mut schedule, *direction, 420.0, *dt);
            }
            let position = world.get::<Position>(player).unwrap().0;
            let previous = world.get::<PreviousPosition>(player).unwrap().0;
            (position, previous)
        };

        let first = run();
        let second = run();
        assert_eq!(
            (first.0.map(f32::to_bits), first.1.map(f32::to_bits)),
            (second.0.map(f32::to_bits), second.1.map(f32::to_bits)),
            "the same steps must reproduce the same bits"
        );
    }
}
