//! Static collision blocks and the `collide` system.
//!
//! The classic scene's level geometry: three static pillars, all of them off
//! the y = 300 corridor the input, camera, and ECS verifications walk along,
//! spawned as ordinary entities and resolved against the player every fixed
//! step.
//!
//! The pieces:
//!
//! - [`CLASSIC_BLOCKS`] — the layout, shared by [`spawn_blocks`] and the
//!   renderer (each block's [`SpriteRenderer`]), so what is drawn is what
//!   collides.
//! - [`spawn_blocks`] — one `Transform2d` + [`Collider`] + [`SpriteRenderer`]
//!   entity per block. No [`Motion`], which is exactly what makes them static.
//! - [`collide_system`] — the schedule's fourth step: broad phase, narrow
//!   phase, then positional resolution, in that order.
//!
//! Resolution is positional and minimal: every overlapping candidate pair is
//! pushed apart along its contact normal, and the correction is absorbed by
//! the bodies that can move — a body with [`Motion`] takes all of it, two
//! movables split it evenly, two static bodies are left alone. One pass per
//! step is enough at this scale: a body wedged between two blocks is
//! corrected against each in turn, and the next step's pass cleans up
//! whatever a single pass could not.

use std::time::Duration;

use mikan::{
    Aabb, Broadphase, Color, Contact, DeadEntity, Entity, Pair, Transform2d, World, contacts,
};

use crate::player::{Collider, Motion};
use crate::renderable::SpriteRenderer;

/// Size of one broad-phase grid cell, in pixels.
///
/// Larger than the widest collider (the 140-pixel pillar), so a block spans
/// at most four cells and candidate pairs stay close to real pairs.
pub const GRID_CELL: f32 = 160.0;

/// Fill color of the classic scene's collision blocks.
///
/// Deliberately cool and dark: the camera and pixel-perfect verifications
/// find the player square by "warm" pixels (red above green above blue), and
/// a block must never satisfy that heuristic.
pub const BLOCK_COLOR: Color = Color::rgb(0.22, 0.32, 0.44);

/// A static block of the classic scene: where it sits and how big it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    /// Center in world pixels.
    pub center: [f32; 2],
    /// Full width and height in pixels.
    pub size: [f32; 2],
}

/// The classic scene's blocks.
///
/// All three stay clear of the y = 300 corridor (the 96-pixel player spans
/// y 252..348 there), so the documented straight-line walk the input and
/// camera verifications perform is unchanged by their presence. The first
/// block is the pillar the collision verification walks into diagonally.
pub const CLASSIC_BLOCKS: [Block; 3] = [
    Block {
        center: [700.0, 460.0],
        size: [140.0, 140.0],
    },
    Block {
        center: [300.0, 460.0],
        size: [120.0, 120.0],
    },
    Block {
        center: [480.0, 120.0],
        size: [200.0, 80.0],
    },
];

/// Spawns one entity per [`CLASSIC_BLOCKS`] entry and returns them in layout
/// order.
///
/// # Errors
///
/// Returns [`DeadEntity`] if a freshly spawned entity is somehow not alive;
/// by construction this cannot happen, but the signature keeps the caller
/// from unwrapping a failure path.
pub fn spawn_blocks(world: &mut World) -> Result<Vec<Entity>, DeadEntity> {
    let mut entities = Vec::with_capacity(CLASSIC_BLOCKS.len());
    for block in CLASSIC_BLOCKS {
        let entity = world.spawn();
        world.insert(entity, Transform2d::from_translation(block.center))?;
        world.insert(entity, Collider { size: block.size })?;
        world.insert(entity, SpriteRenderer::solid(block.size, BLOCK_COLOR))?;
        entities.push(entity);
    }
    Ok(entities)
}

/// Builds the `collide` system.
///
/// The returned closure owns its scratch buffers — the grid, the box list,
/// the candidate and contact lists, the per-entity offsets — so a step does
/// not allocate once they have grown to the scene's size.
///
/// Candidate pairs come from the broad phase; the narrow phase classifies
/// them (touching and separated pairs report depth zero); only real overlaps
/// produce a correction. The normal points from the pair's first entity
/// toward its second, so translating the second by `normal * depth` — and
/// the first by the opposite — separates them.
pub fn collide_system() -> impl FnMut(&mut World, Duration) {
    let mut broadphase = Broadphase::new(GRID_CELL);
    let mut boxes: Vec<Aabb> = Vec::new();
    let mut entities: Vec<Entity> = Vec::new();
    let mut pairs: Vec<Pair> = Vec::new();
    let mut results: Vec<Contact> = Vec::new();
    let mut offsets: Vec<[f32; 2]> = Vec::new();

    move |world, _dt| {
        // Collect the colliders first: the iteration borrows the world, and
        // the passes below need it again.
        boxes.clear();
        entities.clear();
        for (entity, transform, collider) in world.iter2::<Transform2d, Collider>() {
            entities.push(entity);
            boxes.push(Aabb::from_center_size(transform.translation, collider.size));
        }
        if entities.len() < 2 {
            return;
        }

        broadphase.rebuild(&boxes);
        broadphase.pairs(&mut pairs);
        results.resize(
            pairs.len(),
            Contact {
                normal: [0.0, 0.0],
                depth: 0.0,
            },
        );
        contacts(&boxes, &pairs, &mut results);

        offsets.clear();
        offsets.resize(entities.len(), [0.0, 0.0]);
        for (pair, contact) in pairs.iter().zip(&results) {
            if !contact.is_overlap() {
                continue;
            }
            let first = pair.first as usize;
            let second = pair.second as usize;
            let push = [
                contact.normal[0] * contact.depth,
                contact.normal[1] * contact.depth,
            ];
            match (
                world.has::<Motion>(entities[first]),
                world.has::<Motion>(entities[second]),
            ) {
                // Two movables split the correction evenly; a single movable
                // absorbs all of it; two static bodies never overlap in this
                // scene, but leaving them alone is the safe answer.
                (true, true) => {
                    add(&mut offsets[first], negate(half(push)));
                    add(&mut offsets[second], half(push));
                }
                (true, false) => add(&mut offsets[first], negate(push)),
                (false, true) => add(&mut offsets[second], push),
                (false, false) => {}
            }
        }

        for (entity, offset) in entities.iter().zip(&offsets) {
            if offset[0] == 0.0 && offset[1] == 0.0 {
                continue;
            }
            if let Some(transform) = world.get_mut::<Transform2d>(*entity) {
                transform.translation[0] += offset[0];
                transform.translation[1] += offset[1];
            }
        }
    }
}

fn add(into: &mut [f32; 2], delta: [f32; 2]) {
    into[0] += delta[0];
    into[1] += delta[1];
}

fn half(value: [f32; 2]) -> [f32; 2] {
    [value[0] * 0.5, value[1] * 0.5]
}

fn negate(value: [f32; 2]) -> [f32; 2] {
    [-value[0], -value[1]]
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::player::{Controls, Player, WorldBounds, schedule, spawn_player};
    use crate::scene::PLAYER_SPEED;

    /// The classic scene's fixed step: `FixedTimestep::new(120, ..)`.
    fn step_duration() -> Duration {
        Duration::from_secs_f64(1.0 / 120.0)
    }

    fn translation(world: &World, entity: Entity) -> [f32; 2] {
        world
            .get::<Transform2d>(entity)
            .expect("a transform")
            .translation
    }

    fn fixture() -> (World, Entity, Vec<Entity>) {
        let mut world = World::new();
        world.insert_resource(WorldBounds([960.0, 600.0]));
        world.insert_resource(Controls {
            direction: [0.0, 0.0],
            speed: 0.0,
        });
        let player = spawn_player(&mut world, [480.0, 300.0], 96.0, PLAYER_SPEED)
            .expect("a fresh entity is alive");
        let blocks = spawn_blocks(&mut world).expect("a fresh entity is alive");
        (world, player, blocks)
    }

    #[test]
    fn blocks_spawn_as_static_transform_collider_entities() {
        let mut world = World::new();
        let entities = spawn_blocks(&mut world).expect("a fresh entity is alive");
        assert_eq!(entities.len(), CLASSIC_BLOCKS.len());
        for (entity, block) in entities.iter().zip(CLASSIC_BLOCKS) {
            assert_eq!(translation(&world, *entity), block.center);
            assert_eq!(world.get::<Collider>(*entity).unwrap().size, block.size);
            assert!(world.has::<SpriteRenderer>(*entity), "blocks are drawable");
            assert!(!world.has::<Motion>(*entity), "blocks never move");
            assert!(!world.has::<Player>(*entity), "blocks are not the player");
        }
        assert_eq!(world.entity_count(), CLASSIC_BLOCKS.len());
    }

    #[test]
    fn blocks_stay_clear_of_the_documented_corridor() {
        let half = crate::scene::PLAYER_SIZE * 0.5;
        for block in CLASSIC_BLOCKS {
            let top = block.center[1] - block.size[1] * 0.5;
            let bottom = block.center[1] + block.size[1] * 0.5;
            assert!(
                bottom <= 300.0 - half || top >= 300.0 + half,
                "the block at {:?} overlaps the y = 300 corridor the input \
                 and camera verifications walk",
                block.center,
            );
        }
    }

    #[test]
    fn separated_entities_are_left_alone() {
        let (mut world, player, blocks) = fixture();
        let mut collide = collide_system();
        collide(&mut world, step_duration());

        assert_eq!(translation(&world, player), [480.0, 300.0]);
        for (entity, block) in blocks.iter().zip(CLASSIC_BLOCKS) {
            assert_eq!(translation(&world, *entity), block.center);
        }
    }

    #[test]
    fn a_movable_body_is_pushed_clear_of_a_block() {
        let mut world = World::new();
        // Directly inside the pillar's footprint: x overlap 96, y overlap 78,
        // so the shallow axis is y and the player exits through the top.
        let player = spawn_player(&mut world, [700.0, 420.0], 96.0, PLAYER_SPEED)
            .expect("a fresh entity is alive");
        let blocks = spawn_blocks(&mut world).expect("a fresh entity is alive");

        let mut collide = collide_system();
        collide(&mut world, step_duration());

        assert_eq!(
            translation(&world, player),
            [700.0, 342.0],
            "pushed out along the shallow axis, to the pillar's top edge"
        );
        assert_eq!(
            translation(&world, blocks[0]),
            CLASSIC_BLOCKS[0].center,
            "the block absorbs nothing"
        );
    }

    #[test]
    fn two_movable_entities_split_the_correction_evenly() {
        let mut world = World::new();
        let first =
            spawn_player(&mut world, [400.0, 300.0], 96.0, 420.0).expect("a fresh entity is alive");
        let second =
            spawn_player(&mut world, [430.0, 300.0], 96.0, 420.0).expect("a fresh entity is alive");

        let mut collide = collide_system();
        collide(&mut world, step_duration());

        // 66 px of x overlap split in half: 400 - 33 and 430 + 33.
        assert_eq!(translation(&world, first), [367.0, 300.0]);
        assert_eq!(translation(&world, second), [463.0, 300.0]);
    }

    #[test]
    fn the_diagonal_run_settles_against_the_pillar() {
        let (mut world, player, blocks) = fixture();
        let mut schedule = schedule();
        let direction = [core::f32::consts::FRAC_1_SQRT_2; 2];

        // Two seconds of held down+right at 120 Hz: enough to cross the
        // scene, meet the pillar's left face, and reach the floor clamp.
        for _ in 0..240 {
            world.insert_resource(Controls {
                direction,
                speed: PLAYER_SPEED,
            });
            schedule.run(&mut world, step_duration());
        }

        let position = translation(&world, player);
        let pillar = CLASSIC_BLOCKS[0];
        let face = pillar.center[0] - pillar.size[0] * 0.5 - 96.0 * 0.5;
        assert!(
            (position[0] - face).abs() < 0.01,
            "the player rests against the pillar's left face at {face}, not {}",
            position[0],
        );
        assert_eq!(position[1], 552.0, "the floor clamp holds the player");
        assert_eq!(
            translation(&world, blocks[0]),
            pillar.center,
            "the pillar never moves"
        );
    }
}
