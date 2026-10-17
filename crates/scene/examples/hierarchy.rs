//! A scene with a real hierarchy: a sun, three planets, and a moon — the
//! shape `games/demo2d` uses on screen, minus the window.
//!
//! Run with `cargo run -p mikan-scene --example hierarchy`.

use std::time::Duration;

use mikan_ecs::Entity;
use mikan_scene::{GlobalTransform2d, Scene, Transform2d};

/// Angular velocity in radians per second; only entities that spin carry it.
struct Spin(f32);

const ORBIT_RADIUS: f32 = 160.0;
const MOON_RADIUS: f32 = 80.0;

fn main() {
    let mut scene = Scene::new("hierarchy");

    scene.schedule_mut().add("spin", |world, dt| {
        for (_, transform, spin) in world.iter2_mut::<Transform2d, Spin>() {
            transform.rotation += spin.0 * dt.as_secs_f32();
        }
    });

    let sun = scene.spawn();
    scene
        .world_mut()
        .insert(sun, Transform2d::default())
        .unwrap();
    scene.world_mut().insert(sun, Spin(0.5)).unwrap();

    let mut planets = Vec::new();
    for step in 0..3 {
        let angle = step as f32 * std::f32::consts::TAU / 3.0;
        let planet = scene.spawn();
        scene
            .world_mut()
            .insert(
                planet,
                Transform2d::from_translation([
                    ORBIT_RADIUS * angle.cos(),
                    ORBIT_RADIUS * angle.sin(),
                ]),
            )
            .unwrap();
        scene.set_parent(planet, sun).unwrap();
        planets.push(planet);
    }

    let moon = scene.spawn();
    scene
        .world_mut()
        .insert(moon, Transform2d::from_translation([MOON_RADIUS, 0.0]))
        .unwrap();
    scene.set_parent(moon, planets[0]).unwrap();

    println!("systems: {:?}", scene.systems().collect::<Vec<_>>());
    for step in 0..3 {
        scene.update(Duration::from_millis(500));
        let radius = orbit_radius(&scene, planets[0], sun);
        println!(
            "t={:.1}s  planet 0 at {:?} (orbit radius {radius:.3}), moon at {:?}",
            (step + 1) as f32 * 0.5,
            position(&scene, planets[0]),
            position(&scene, moon),
        );
    }
    assert!(
        (orbit_radius(&scene, planets[0], sun) - ORBIT_RADIUS).abs() < 0.01,
        "the orbit radius is preserved when the parent rotates"
    );
}

fn position(scene: &Scene, entity: Entity) -> [f32; 2] {
    scene
        .world()
        .get::<GlobalTransform2d>(entity)
        .expect("propagated")
        .translation()
}

fn orbit_radius(scene: &Scene, entity: Entity, center: Entity) -> f32 {
    let a = position(scene, entity);
    let b = position(scene, center);
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}
