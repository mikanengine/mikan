//! Scenes and the entity transform hierarchy for Mikan.
//!
//! The scene system is the layer between the ECS (`mikan-ecs`) and the
//! engine's frame loop. It answers four questions:
//!
//! - **Where is each entity?** [`Transform2d`] is the local transform
//!   (translation, rotation, scale) an entity carries; [`GlobalTransform2d`]
//!   is the derived world-space result of composing it with every ancestor.
//! - **What is each entity's parent?** [`Hierarchy`] is a forest over entity
//!   handles: at most one parent per entity, no cycles, and destroyed
//!   entities leave no dangling edges. Topology and propagation order live in
//!   the native scene module (`mikan-scene-sys`); this crate is the safe
//!   wrapper and adds the engine semantics.
//! - **What is a scene?** [`Scene`] owns one [`World`], one [`Schedule`], and
//!   the update order: run the systems, flush deferred despawns, then
//!   recompute world transforms.
//! - **How do scenes coexist?** [`SceneManager`] owns many scenes, hands out
//!   generational [`SceneId`]s, and runs the one active scene.
//!
//! # The update order is the contract
//!
//! [`Scene::update`] does exactly three things, in order:
//!
//! 1. **Systems**: `schedule.run(world, dt)` — once per fixed step, in
//!    registration order, exactly as `mikan-ecs` defines.
//! 2. **Deferred despawns**: [`flush_despawns`] applies every
//!    [`request_despawn`] made during the step, removing each entity from the
//!    hierarchy and the world. This is how systems destroy entities without
//!    holding a mutable borrow of the world mid-iteration.
//! 3. **Transform propagation**: [`propagate_transforms`] prunes dead nodes,
//!    reconciles entities that gained a [`Transform2d`], composes world
//!    transforms parents-first through the native kernel, and writes one
//!    [`GlobalTransform2d`] per hierarchy entity.
//!
//! Because propagation runs **after** the systems, a system that reads
//! [`GlobalTransform2d`] sees the worlds of the *previous* step (a one-step
//! lag for entities it just moved), while readers outside the schedule — the
//! renderer, tests, gameplay queries after `update` — always see the current
//! step. That is the documented trade: systems move local state, and the
//! derived cache is refreshed once per step in one deterministic place.
//!
//! # Local state, derived result
//!
//! [`Transform2d`] is the simulation state: systems mutate it, the engine
//! never does. [`GlobalTransform2d`] is a cache rebuilt from scratch every
//! [`propagate_transforms`] call; writing to it directly is meaningless
//! because the next propagation overwrites it. Destroying an entity (or
//! removing its `Transform2d`) therefore cannot leave a stale world
//! transform behind for a renderer: the entity's components are gone, and
//! despawn removes its hierarchy node.
//!
//! # Hierarchy edits keep locals
//!
//! Attaching, detaching, or destroying a parent never rewrites any
//! [`Transform2d`]: entities are placed where their local transform says, and
//! a child that loses its parent simply becomes a root with the same local
//! transform. Reparenting with a world-space-preserving bake is a gameplay
//! operation (read [`GlobalTransform2d`], write the computed local), not a
//! hidden side effect of topology changes.
//!
//! # Example
//!
//! ```
//! use std::time::Duration;
//!
//! use mikan_ecs::Entity;
//! use mikan_scene::{GlobalTransform2d, Scene, Transform2d};
//!
//! let mut scene = Scene::new("demo");
//! scene.schedule_mut().add("spin", |world, dt| {
//!     for (_, transform) in world.iter_mut::<Transform2d>() {
//!         transform.rotation += dt.as_secs_f32();
//!     }
//! });
//!
//! let sun: Entity = scene.spawn();
//! scene.world_mut().insert(sun, Transform2d::default())?;
//! let planet = scene.spawn();
//! scene
//!     .world_mut()
//!     .insert(planet, Transform2d::from_translation([10.0, 0.0]))?;
//! scene.set_parent(planet, sun)?;
//!
//! scene.update(Duration::from_millis(100));
//!
//! let world = scene.world();
//! let planet = world.get::<GlobalTransform2d>(planet).expect("propagated");
//! // The sun rotated 0.1 rad, so the planet now orbits it.
//! assert!(planet.translation()[0] < 10.0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Determinism
//!
//! For a given binary and target, the same scene run with the same input
//! sequence performs the same work in the same order: systems in registration
//! order, despawns in request order, hierarchy nodes in insertion order, and
//! the C kernel's results are bit-identical to the Rust reference in
//! [`reference`] (cross-checked by tests). Floating-point results are not
//! promised bit-identical *across* architectures or compilers, matching the
//! ECS's determinism note.

pub mod manager;
pub mod reference;
pub mod scene;
pub mod transform;

mod despawn;
mod hierarchy;

pub use despawn::{DespawnQueue, flush_despawns, request_despawn};
pub use hierarchy::{Hierarchy, HierarchyError};
pub use manager::{SceneError, SceneId, SceneManager};
pub use scene::{Scene, propagate_transforms};
pub use transform::{GlobalTransform2d, Transform2d};
