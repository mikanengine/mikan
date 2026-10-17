//! The `--scene hierarchy` demonstration: a sun, planets, and a moon linked by
//! the scene's transform hierarchy.
//!
//! Every body is an ordinary entity with a local [`Transform2d`] and a
//! [`SpriteRenderer`]; the only thing that makes the scene a hierarchy is the
//! parent links:
//!
//! - the **sun** is a root at the centre of the viewport;
//! - [`PLANET_COUNT`] **planets** are children of the sun, each at
//!   [`ORBIT_RADIUS`] on its own angle, so a planet's world position is its
//!   parent's frame applied to its local offset;
//! - the **moon** is a child of the first planet at [`MOON_ORBIT_RADIUS`].
//!
//! Holding `A` or `D` spins the sun (through the [`Spin`] resource the demo
//! writes each step); because the planets and the moon are children, the whole
//! system orbits without a single line of per-body orbit code — that is the
//! point of the hierarchy. Pressing `F` removes the moon: [`Scene`] queues the
//! despawn and applies it before the next propagation.
//!
//! Positions are exact at the initial angles, which is what the capture
//! verification pins:
//!
//! - the sun's centre is at `[480, 300]`;
//! - the first planet (angle `0`) is at `[640, 300]`, [`ORBIT_RADIUS`] to the
//!   right;
//! - the moon is at `[720, 300]`, `ORBIT_RADIUS + MOON_ORBIT_RADIUS` to the
//!   right, so the two offsets add along one axis.

use core::f32::consts::TAU;

use mikan::{DeadEntity, Entity, HierarchyError, Scene, Transform2d, UvRect};

use crate::renderable::SpriteRenderer;

/// Centre of the sun in world pixels.
pub const SUN_CENTER: [f32; 2] = [480.0, 300.0];
/// Edge length of the sun, in pixels.
pub const SUN_SIZE: f32 = 96.0;
/// Edge length of a planet, in pixels.
pub const PLANET_SIZE: f32 = 48.0;
/// Edge length of the moon, in pixels.
pub const MOON_SIZE: f32 = 24.0;
/// Distance of every planet from the sun, in pixels.
pub const ORBIT_RADIUS: f32 = 160.0;
/// Distance of the moon from its planet, in pixels.
pub const MOON_ORBIT_RADIUS: f32 = 80.0;
/// Number of planets orbiting the sun.
pub const PLANET_COUNT: usize = 3;
/// Angular speed the sun spins at while a spin key is held, in radians/second.
pub const SPIN_SPEED: f32 = 1.2;

/// The sun's angular velocity this step, in radians/second (a resource).
///
/// Positive values turn clockwise on screen (world `y` grows downward). The
/// demo writes it every fixed step from the held keys; a missing resource
/// means "not spinning".
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Spin(pub f32);

/// Handles to the parts of the hierarchy the demo needs afterwards.
#[derive(Debug, Clone, Copy)]
pub struct HierarchyScene {
    sun: Entity,
    planets: [Entity; PLANET_COUNT],
    moon: Entity,
}

impl HierarchyScene {
    /// The sun, the one spinning root.
    #[must_use]
    pub const fn sun(&self) -> Entity {
        self.sun
    }

    /// The `index`-th planet, in orbit-angle order (planet 0 is at angle `0`).
    ///
    /// # Panics
    ///
    /// Panics if `index >= PLANET_COUNT`.
    #[must_use]
    pub const fn planet(&self, index: usize) -> Entity {
        self.planets[index]
    }

    /// The moon, the leaf that `F` removes.
    #[must_use]
    pub const fn moon(&self) -> Entity {
        self.moon
    }
}

/// Why the hierarchy scene could not be built.
#[derive(Debug, thiserror::Error)]
pub enum HierarchySetupError {
    /// A freshly spawned entity was somehow not alive.
    #[error(transparent)]
    Ecs(#[from] DeadEntity),

    /// A parent link could not be made.
    #[error(transparent)]
    Hierarchy(#[from] HierarchyError),
}

/// Populates `scene` with the sun, its planets, and the moon, and registers the
/// `spin` system.
///
/// `player_uv` is the atlas frame the sun and planets draw with and `marker_uv`
/// the frame the planets use; passing them in rather than an [`Assets`] keeps
/// this function testable without a GPU.
///
/// # Errors
///
/// Returns [`HierarchySetupError`] only if the engine's own invariants are
/// broken (a fresh entity is dead, or a link between two live entities fails).
pub fn spawn(
    scene: &mut Scene,
    player_uv: UvRect,
    marker_uv: UvRect,
) -> Result<HierarchyScene, HierarchySetupError> {
    let sun = scene.spawn();
    scene
        .world_mut()
        .insert(sun, Transform2d::from_translation(SUN_CENTER))?;
    scene
        .world_mut()
        .insert(sun, SpriteRenderer::atlas([SUN_SIZE; 2], player_uv))?;

    let mut planets = [sun; PLANET_COUNT];
    for (index, planet) in planets.iter_mut().enumerate() {
        let angle = index as f32 * TAU / PLANET_COUNT as f32;
        let offset = [ORBIT_RADIUS * angle.cos(), ORBIT_RADIUS * angle.sin()];
        *planet = scene.spawn();
        scene
            .world_mut()
            .insert(*planet, Transform2d::from_parts(offset, angle, [1.0, 1.0]))?;
        scene
            .world_mut()
            .insert(*planet, SpriteRenderer::atlas([PLANET_SIZE; 2], marker_uv))?;
        scene.set_parent(*planet, sun)?;
    }

    let moon = scene.spawn();
    scene.world_mut().insert(
        moon,
        Transform2d::from_translation([MOON_ORBIT_RADIUS, 0.0]),
    )?;
    scene
        .world_mut()
        .insert(moon, SpriteRenderer::star([MOON_SIZE; 2]))?;
    scene.set_parent(moon, planets[0])?;

    scene.schedule_mut().add("spin", move |world, dt| {
        let spin = world.get_resource::<Spin>().copied().unwrap_or_default().0;
        if spin == 0.0 {
            return;
        }
        if let Some(transform) = world.get_mut::<Transform2d>(sun) {
            transform.rotation += spin * dt.as_secs_f32();
        }
    });

    Ok(HierarchyScene { sun, planets, moon })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    use mikan::GlobalTransform2d;

    fn scene() -> (Scene, HierarchyScene) {
        let mut scene = Scene::new("hierarchy");
        let bodies =
            spawn(&mut scene, UvRect::FULL, UvRect::FULL).expect("a fresh hierarchy is buildable");
        scene.propagate();
        (scene, bodies)
    }

    fn translation(scene: &Scene, entity: Entity) -> [f32; 2] {
        scene
            .world()
            .get::<GlobalTransform2d>(entity)
            .expect("propagated")
            .translation()
    }

    fn distance_from_sun(scene: &Scene, bodies: &HierarchyScene, body: Entity) -> f32 {
        let sun = translation(scene, bodies.sun());
        let body = translation(scene, body);
        (body[0] - sun[0]).hypot(body[1] - sun[1])
    }

    #[test]
    fn the_initial_layout_is_exact() {
        let (scene, bodies) = scene();
        assert_eq!(translation(&scene, bodies.sun()), SUN_CENTER);
        assert_eq!(translation(&scene, bodies.planet(0)), [640.0, 300.0]);
        assert_eq!(translation(&scene, bodies.moon()), [720.0, 300.0]);
        assert_eq!(
            distance_from_sun(&scene, &bodies, bodies.moon()),
            ORBIT_RADIUS + MOON_ORBIT_RADIUS
        );
        assert_eq!(scene.entity_count(), 1 + PLANET_COUNT + 1);
    }

    #[test]
    fn the_links_are_the_ones_the_scene_documents() {
        let (scene, bodies) = scene();
        assert_eq!(scene.parent(bodies.planet(0)).unwrap(), Some(bodies.sun()));
        assert_eq!(scene.parent(bodies.planet(1)).unwrap(), Some(bodies.sun()));
        assert_eq!(scene.parent(bodies.planet(2)).unwrap(), Some(bodies.sun()));
        assert_eq!(scene.parent(bodies.moon()).unwrap(), Some(bodies.planet(0)));
        assert_eq!(scene.parent(bodies.sun()).unwrap(), None);
    }

    #[test]
    fn spinning_the_sun_orbits_every_descendant_rigidly() {
        let (mut scene, bodies) = scene();
        scene.world_mut().insert_resource(Spin(SPIN_SPEED));
        scene.update(Duration::from_millis(500));

        // 1.2 rad/s for 0.5 s is 0.6 rad of rotation.
        let sun = scene.world().get::<Transform2d>(bodies.sun()).unwrap();
        assert!((sun.rotation - SPIN_SPEED * 0.5).abs() < 1e-6);
        let planet = translation(&scene, bodies.planet(0));
        assert!(
            planet[0] < 640.0 && planet[1] > 300.0,
            "orbit moved: {planet:?}"
        );
        // Rigid rotation preserves the distances.
        let distance = distance_from_sun(&scene, &bodies, bodies.planet(0));
        assert!((distance - ORBIT_RADIUS).abs() < 1e-3);
        let moon = distance_from_sun(&scene, &bodies, bodies.moon());
        assert!((moon - (ORBIT_RADIUS + MOON_ORBIT_RADIUS)).abs() < 1e-3);
    }

    #[test]
    fn despawning_the_moon_leaves_the_rest_alone() {
        let (mut scene, bodies) = scene();
        scene.despawn_later(bodies.moon());
        scene.update(Duration::ZERO);
        assert!(!scene.world().is_alive(bodies.moon()));
        assert!(scene.world().is_alive(bodies.sun()));
        assert!(scene.world().is_alive(bodies.planet(0)));
        assert_eq!(translation(&scene, bodies.planet(0)), [640.0, 300.0]);
    }

    #[test]
    fn no_spin_resource_keeps_the_sun_still() {
        let (mut scene, bodies) = scene();
        scene.update(Duration::from_millis(500));
        assert_eq!(
            translation(&scene, bodies.sun()),
            SUN_CENTER,
            "without the resource the sun does not turn"
        );
    }
}
