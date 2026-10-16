//! Integration tests through the public API: a whole schedule run across a
//! real world, and the reproduction guarantees the crate documentation makes.
//!
//! Everything here uses only `mikan_ecs` exports — no internal hooks — so it
//! exercises the same surface an application does.

use std::time::Duration;

use mikan_ecs::{Entity, Schedule, World};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Position([f32; 2]);

#[derive(Debug, Clone, Copy, PartialEq)]
struct Velocity([f32; 2]);

#[derive(Debug, Clone, Copy, PartialEq)]
struct Player;

/// Builds the same handful of entities every time: one player that moves, one
/// entity with a position only, and one with a velocity only.
fn build_world() -> (World, Entity) {
    let mut world = World::new();
    let player = world.spawn();
    world.insert(player, Position([0.0, 0.0])).unwrap();
    world.insert(player, Velocity([90.0, -30.0])).unwrap();
    world.insert(player, Player).unwrap();

    let stationary = world.spawn();
    world.insert(stationary, Position([5.0, 5.0])).unwrap();

    let ghost = world.spawn();
    world.insert(ghost, Velocity([1.0, 1.0])).unwrap();

    (world, player)
}

/// The movement schedule under test: exactly one write per fixed step.
fn build_schedule() -> Schedule {
    let mut schedule = Schedule::new();
    schedule.add("move", |world, dt| {
        for (_, position, velocity) in world.iter2_mut::<Position, Velocity>() {
            position.0[0] += velocity.0[0] * dt.as_secs_f32();
            position.0[1] += velocity.0[1] * dt.as_secs_f32();
        }
    });
    schedule
}

/// Runs `steps` fixed steps of `1/hz` seconds each.
fn run(world: &mut World, schedule: &mut Schedule, steps: u32, hz: u32) {
    let step = Duration::from_secs_f64(1.0 / f64::from(hz));
    for _ in 0..steps {
        schedule.run(world, step);
    }
}

#[test]
fn identical_runs_produce_bit_identical_positions() {
    let (mut first_world, first_player) = build_world();
    let (mut second_world, second_player) = build_world();
    let (mut first_schedule, mut second_schedule) = (build_schedule(), build_schedule());

    run(&mut first_world, &mut first_schedule, 600, 120);
    run(&mut second_world, &mut second_schedule, 600, 120);

    let first = first_world.get::<Position>(first_player).unwrap();
    let second = second_world.get::<Position>(second_player).unwrap();
    assert_eq!(
        first.0.map(f32::to_bits),
        second.0.map(f32::to_bits),
        "the same steps must reproduce the same bits: {first:?} vs {second:?}"
    );
}

#[test]
fn a_full_run_matches_the_closed_form_distance() {
    let (mut world, player) = build_world();
    let mut schedule = build_schedule();

    // 600 steps of 1/120 s is 5 s at 90/-30 units/s.
    run(&mut world, &mut schedule, 600, 120);

    let position = world.get::<Position>(player).unwrap().0;
    let expected = [90.0 * 5.0, -30.0 * 5.0];
    for axis in 0..2 {
        assert!(
            (position[axis] - expected[axis]).abs() < 1e-2,
            "axis {axis}: expected ~{}, got {}",
            expected[axis],
            position[axis]
        );
    }
}

#[test]
fn entities_without_both_components_are_never_touched() {
    let (mut world, player) = build_world();
    let mut schedule = build_schedule();

    let entities: Vec<Entity> = world.iter::<Position>().map(|(entity, _)| entity).collect();
    assert_eq!(entities.len(), 2, "the player and the stationary entity");

    run(&mut world, &mut schedule, 10, 120);

    let player_position = world.get::<Position>(player).unwrap().0;
    assert_ne!(player_position, [0.0, 0.0], "the player moved");

    // The position-only entity did not move, and iteration skipped the
    // velocity-only ghost entirely.
    let moved: Vec<Position> = world
        .iter::<Position>()
        .filter(|(entity, _)| *entity != player)
        .map(|(_, position)| *position)
        .collect();
    assert_eq!(moved, [Position([5.0, 5.0])]);
}

#[test]
fn despawn_and_respawn_do_not_alias_state() {
    let mut world = World::new();
    let first = world.spawn();
    world.insert(first, Position([1.0, 1.0])).unwrap();
    world.insert(first, Player).unwrap();
    assert!(world.despawn(first));

    let second = world.spawn();
    assert_eq!(second.index(), first.index(), "the slot is reused");
    assert!(!world.is_alive(first), "the old handle stays dead");

    // The new entity starts with no components; nothing leaked from the old
    // occupant, and the stale handle cannot reach the new one.
    assert!(!world.has::<Position>(second));
    assert!(!world.has::<Player>(second));
    assert!(world.get::<Position>(first).is_none());
    assert!(world.insert(first, Position([9.0, 9.0])).is_err());

    // The new occupant works normally.
    world.insert(second, Position([2.0, 2.0])).unwrap();
    assert_eq!(world.get::<Position>(second), Some(&Position([2.0, 2.0])));
}

#[test]
fn iteration_order_is_insertion_order_across_reuse() {
    let mut world = World::new();
    let first = world.spawn();
    let second = world.spawn();
    let third = world.spawn();
    for (entity, x) in [(first, 1.0), (second, 2.0), (third, 3.0)] {
        world.insert(entity, Position([x, 0.0])).unwrap();
    }

    world.despawn(second);
    let reused = world.spawn();
    world.insert(reused, Position([4.0, 0.0])).unwrap();

    let order: Vec<f32> = world.iter::<Position>().map(|(_, p)| p.0[0]).collect();
    assert_eq!(
        order,
        [1.0, 3.0, 4.0],
        "first stays first, third keeps its place, the new entry appends"
    );
}
