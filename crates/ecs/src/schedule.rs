//! Explicitly ordered systems: the [`Schedule`].
//!
//! A schedule is a list of named systems run in registration order. There is
//! no dependency graph, no parallelism, and no implicit reordering: the order
//! you add systems in is the order they execute, and that is the whole
//! scheduling model. When order matters — snapshot before integration, input
//! before movement — it is stated in one visible place, and the names make
//! that place readable in logs and tests.
//!
//! # The fixed timestep is the caller's
//!
//! The ECS deliberately has no timer of its own. A system receives exactly
//! the `dt` its [`Schedule::run`] call was given, and the established pattern
//! in Mikan is to run the schedule once per fixed update:
//!
//! ```
//! use std::time::Duration;
//!
//! use mikan_ecs::{Schedule, World};
//!
//! let mut world = World::new();
//! let mut schedule = Schedule::new();
//! schedule.add("tick", |_world, _dt| { /* one fixed step */ });
//!
//! // `Game::update` is called by the platform runner once per fixed step,
//! // with `dt` equal to the step duration; drive the schedule from there.
//! let fixed_step = Duration::from_secs_f64(1.0 / 120.0);
//! schedule.run(&mut world, fixed_step);
//! ```
//!
//! Because each `run` executes every system exactly once, simulation state is
//! never accidentally updated twice per frame: the platform's fixed-timestep
//! scheduler decides how many steps a frame gets, and each step runs the
//! schedule exactly once.

use core::fmt;
use core::time::Duration;

use crate::World;

/// A system: world in, fixed step out, no return value.
///
/// Systems are closure-shaped so they can capture their own scratch state and
/// configuration. They are infallible: work that can fail (I/O, device
/// operations) belongs in an application hook, which can return an error; a
/// system that needs to report a failure can record it in a resource the
/// caller inspects.
type SystemFn = dyn FnMut(&mut World, Duration);

struct System {
    name: String,
    run: Box<SystemFn>,
}

/// A named, explicitly ordered list of systems.
///
/// ```
/// use mikan_ecs::{Schedule, World};
///
/// let mut schedule = Schedule::new();
/// schedule
///     .add("first", |world, _dt| { world.insert_resource(1_u8); })
///     .add("second", |world, _dt| {
///         *world.get_resource_mut::<u8>().expect("first ran before second") += 1;
///     });
///
/// let mut world = World::new();
/// schedule.run(&mut world, std::time::Duration::ZERO);
/// assert_eq!(world.get_resource::<u8>(), Some(&2));
/// ```
#[derive(Default)]
pub struct Schedule {
    systems: Vec<System>,
}

impl Schedule {
    /// Creates an empty schedule.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a system, returning `self` for chaining.
    ///
    /// The name is a diagnostic label: it appears in [`Schedule::names`] and
    /// in logs. Names do not have to be unique and do not affect execution
    /// order — the order systems were added in is the order they run.
    pub fn add(
        &mut self,
        name: impl Into<String>,
        system: impl FnMut(&mut World, Duration) + 'static,
    ) -> &mut Self {
        self.systems.push(System {
            name: name.into(),
            run: Box::new(system),
        });
        self
    }

    /// Runs every system once, in registration order, with the same `dt`.
    ///
    /// # Determinism
    ///
    /// There is no parallelism and no hidden state: running the same schedule
    /// over the same world with the same sequence of `dt` values repeats the
    /// same work in the same order. Results are reproducible for a given
    /// binary and target; floating-point results are not promised to be
    /// bit-identical across architectures or compilers.
    pub fn run(&mut self, world: &mut World, dt: Duration) {
        for system in &mut self.systems {
            (system.run)(world, dt);
        }
    }

    /// The names of the systems, in execution order.
    pub fn names(&self) -> impl ExactSizeIterator<Item = &str> {
        self.systems.iter().map(|system| system.name.as_str())
    }

    /// Number of systems in the schedule.
    #[must_use]
    pub fn len(&self) -> usize {
        self.systems.len()
    }

    /// Whether the schedule has no systems.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.systems.is_empty()
    }

    /// Removes every system.
    pub fn clear(&mut self) {
        self.systems.clear();
    }
}

impl fmt::Debug for Schedule {
    /// Systems are opaque closures, so the summary lists their names.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.names()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn systems_run_in_registration_order() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut schedule = Schedule::new();
        for name in ["snapshot", "input", "movement"] {
            let log = Rc::clone(&log);
            schedule.add(name, move |_world, _dt| log.borrow_mut().push(name));
        }
        let mut world = World::new();
        schedule.run(&mut world, Duration::ZERO);
        assert_eq!(*log.borrow(), ["snapshot", "input", "movement"]);
    }

    #[test]
    fn a_later_system_sees_earlier_writes() {
        let mut schedule = Schedule::new();
        schedule
            .add("spawn", |world, _dt| {
                let entity = world.spawn();
                world.insert(entity, 21_u32).unwrap();
            })
            .add("double", |world, _dt| {
                for (_, value) in world.iter_mut::<u32>() {
                    *value *= 2;
                }
            });
        let mut world = World::new();
        schedule.run(&mut world, Duration::ZERO);
        let values: Vec<u32> = world.iter::<u32>().map(|(_, value)| *value).collect();
        assert_eq!(values, [42]);
    }

    #[test]
    fn dt_is_forwarded_to_every_system_exactly() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut schedule = Schedule::new();
        for _ in 0..2 {
            let seen = Rc::clone(&seen);
            schedule.add("record", move |_world, dt| seen.borrow_mut().push(dt));
        }
        let mut world = World::new();
        let step = Duration::from_millis(7);
        schedule.run(&mut world, step);
        assert_eq!(*seen.borrow(), [step, step]);
    }

    #[test]
    fn names_reflect_registration_order() {
        let mut schedule = Schedule::new();
        schedule
            .add("a", |_world, _dt| {})
            .add("b", |_world, _dt| {})
            .add("c", |_world, _dt| {});
        assert_eq!(schedule.names().collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(schedule.len(), 3);
        assert!(!schedule.is_empty());
    }

    #[test]
    fn clear_removes_every_system() {
        let mut schedule = Schedule::new();
        schedule.add("a", |_world, _dt| {});
        schedule.clear();
        assert!(schedule.is_empty());
        assert_eq!(schedule.names().count(), 0);
        // A cleared schedule still runs (as a no-op).
        schedule.run(&mut World::new(), Duration::ZERO);
    }

    #[test]
    fn an_empty_schedule_runs() {
        let mut schedule = Schedule::new();
        assert!(schedule.is_empty());
        schedule.run(&mut World::new(), Duration::from_millis(4));
    }

    #[test]
    fn running_again_repeats_the_work() {
        let count = Rc::new(RefCell::new(0_u32));
        let mut schedule = Schedule::new();
        {
            let count = Rc::clone(&count);
            schedule.add("count", move |_world, _dt| *count.borrow_mut() += 1);
        }
        let mut world = World::new();
        for _ in 0..3 {
            schedule.run(&mut world, Duration::ZERO);
        }
        assert_eq!(*count.borrow(), 3);
    }

    #[test]
    fn systems_may_spawn_and_despawn() {
        let mut schedule = Schedule::new();
        schedule
            .add("spawn", |world, _dt| {
                let entity = world.spawn();
                world.insert(entity, 1_u32).unwrap();
            })
            .add("despawn-tagged", |world, _dt| {
                // Collect first: despawning while iterating would need a
                // mutable borrow of the world inside the loop.
                let entities: Vec<_> = world.iter::<u32>().map(|(entity, _)| entity).collect();
                for entity in entities {
                    world.despawn(entity);
                }
            });
        let mut world = World::new();
        let watched = world.spawn();
        world.insert(watched, 1_u32).unwrap();
        schedule.run(&mut world, Duration::ZERO);
        assert_eq!(
            world.entity_count(),
            0,
            "both the spawned and the watched entity were tagged and despawned"
        );
        assert_eq!(world.component_count::<u32>(), 0);
    }

    #[test]
    fn debug_lists_system_names() {
        let mut schedule = Schedule::new();
        schedule
            .add("a", |_world, _dt| {})
            .add("b", |_world, _dt| {});
        assert_eq!(format!("{schedule:?}"), "[\"a\", \"b\"]");
    }
}
