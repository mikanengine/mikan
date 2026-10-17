//! ECS-lite for Mikan: entity storage and systems with explicit scheduling.
//!
//! Four things live here, all of them CPU-only and free of engine concerns:
//!
//! - [`Entity`]: a generational handle. Despawning and slot reuse cannot make
//!   a stale handle point at a different entity.
//! - [`World`]: entity allocation, one sparse-set storage per component type,
//!   and typed resources for data shared by systems.
//! - [`Schedule`]: named systems run **once each, in registration order**,
//!   with the `dt` the caller passes. Order is stated by the call chain; there
//!   is no dependency graph, no parallelism, and no hidden timer.
//! - [`DeadEntity`]: the one error, returned when a component operation
//!   targets an entity that is not alive.
//!
//! This is deliberately a *lite* ECS: dense component storage with stable
//! insertion-order iteration, and nothing more. It is the storage-and-systems
//! layer the engine's fixed timestep drives; there is no archetype
//! bookkeeping, no query caching, no change detection, and no multi-threading.
//! See `docs/ecs.md` for the user guide and `docs/adr/0009-ecs-lite.md` for
//! the decision record, including why this milestone adds no native module.
//!
//! # Driving it from the engine
//!
//! Systems are meant to run inside a fixed update. `mikan-platform` already
//! decides how many fixed steps a frame performs (bounded catch-up on the
//! existing [`FixedTimestep`](https://docs.rs/mikan-core)); an application
//! runs the schedule once per `Game::update` call:
//!
//! ```
//! use std::time::Duration;
//!
//! use mikan_ecs::{Entity, Schedule, World};
//!
//! struct Position([f32; 2]);
//! struct Velocity([f32; 2]);
//!
//! struct Input([f32; 2]); // a resource: shared, owned by no entity
//!
//! let mut world = World::new();
//! world.insert_resource(Input([1.0, 0.0]));
//!
//! let player: Entity = world.spawn();
//! world.insert(player, Position([0.0, 0.0]))?;
//! world.insert(player, Velocity([60.0, 0.0]))?;
//!
//! let mut schedule = Schedule::new();
//! schedule
//!     .add("input", |world, _dt| {
//!         let input = world.get_resource::<Input>().map_or([0.0, 0.0], |i| i.0);
//!         for (_, velocity) in world.iter_mut::<Velocity>() {
//!             velocity.0 = [input[0] * 60.0, input[1] * 60.0];
//!         }
//!     })
//!     .add("move", |world, dt| {
//!         for (_, position, velocity) in world.iter2_mut::<Position, Velocity>() {
//!             position.0[0] += velocity.0[0] * dt.as_secs_f32();
//!             position.0[1] += velocity.0[1] * dt.as_secs_f32();
//!         }
//!     });
//!
//! // One `Game::update` call is one fixed step:
//! let step = Duration::from_secs_f64(1.0 / 120.0);
//! for _ in 0..120 {
//!     schedule.run(&mut world, step);
//! }
//!
//! let position = world.get::<Position>(player).expect("the player is alive").0;
//! assert!((position[0] - 60.0).abs() < 1e-3);
//! # Ok::<(), mikan_ecs::DeadEntity>(())
//! ```
//!
//! # Reproduction
//!
//! The same schedule run over the same world with the same `dt` sequence
//! performs the same work in the same order, so simulation results are
//! reproducible for a given binary and target (see [`Schedule::run`] for the
//! exact promise, including the floating-point caveat). Tests in
//! `tests/determinism.rs` exercise that property, rather than assuming it.

pub mod entity;
pub mod error;
pub mod schedule;
pub mod world;

pub use entity::Entity;
pub use error::DeadEntity;
pub use schedule::Schedule;
pub use world::{Iter, Iter2, Iter2Mut, IterMut, World};
