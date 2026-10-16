//! One moving entity, two systems, ten fixed steps — the smallest complete
//! ECS-lite workflow. Run it with:
//!
//!     cargo run -p mikan-ecs --example fixed_steps

use std::time::Duration;

use mikan_ecs::{Entity, Schedule, World};

struct Position(f32);
struct Speed(f32);

/// The movement direction for this step, owned by no entity.
struct Direction(f32);

fn main() -> Result<(), mikan_ecs::DeadEntity> {
    let mut world = World::new();
    world.insert_resource(Direction(1.0));

    let mover: Entity = world.spawn();
    world.insert(mover, Position(0.0))?;
    world.insert(mover, Speed(60.0))?;

    let mut schedule = Schedule::new();
    schedule.add("move", |world, dt| {
        let direction = world.get_resource::<Direction>().map_or(0.0, |d| d.0);
        for (_, position, speed) in world.iter2_mut::<Position, Speed>() {
            position.0 += direction * speed.0 * dt.as_secs_f32();
        }
    });

    // A fixed step, exactly as `Game::update` receives it from the engine.
    let step = Duration::from_secs_f64(1.0 / 60.0);
    for _ in 0..60 {
        schedule.run(&mut world, step);
    }

    let position = world.get::<Position>(mover).expect("still alive").0;
    println!("after 60 steps of 1/60 s at 60 units/s: x = {position:.3}");
    Ok(())
}
