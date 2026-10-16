# The ECS-lite guide

`crates/mikan-ecs` is a deliberately small entity-component-system layer: dense,
typed component storage, generational entity handles, per-world resources, and a
schedule that runs named systems in registration order. It is CPU-only and
depends on nothing in the workspace, so gameplay data can be tested without a
window, a GPU, or the engine runtime.

It is *lite* on purpose. There are no archetypes, no query caching, no change
detection, no parallel executor, and no events. Those are additions for a
concrete, profiled need, not stubs. The decision record, including why the
milestone adds no native module, is
[ADR 0009](adr/0009-ecs-lite.md).

## The four concepts

- **Entity** — a handle: an index plus a generation. Spawning and despawning
  entities never invalidates a live handle, and a despawned handle never
  resolves again, even after its slot is reused.
- **Component** — a `'static` value attached to one entity. Every component type
  has its own storage inside the world.
- **Resource** — one value of a type per world, for state no entity owns: the
  step's input direction, the world bounds, score, configuration.
- **System** — a closure `FnMut(&mut World, Duration)`; **Schedule** — a named,
  explicitly ordered list of systems, run once each per `run` call.

## A complete example

```rust
use std::time::Duration;

use mikan_ecs::{Entity, Schedule, World};

struct Position([f32; 2]);
struct Velocity([f32; 2]);
struct Input([f32; 2]); // a resource: shared, owned by no entity

let mut world = World::new();
world.insert_resource(Input([1.0, 0.0]));

let player: Entity = world.spawn();
world.insert(player, Position([0.0, 0.0]))?;
world.insert(player, Velocity([60.0, 0.0]))?;

let mut schedule = Schedule::new();
schedule
    .add("input", |world, _dt| {
        let input = world.get_resource::<Input>().map_or([0.0, 0.0], |i| i.0);
        for (_, velocity) in world.iter_mut::<Velocity>() {
            velocity.0 = [input[0] * 60.0, input[1] * 60.0];
        }
    })
    .add("move", |world, dt| {
        for (_, position, velocity) in world.iter2_mut::<Position, Velocity>() {
            position.0[0] += velocity.0[0] * dt.as_secs_f32();
            position.0[1] += velocity.0[1] * dt.as_secs_f32();
        }
    });

// One `Game::update` call is one fixed step:
let step = Duration::from_secs_f64(1.0 / 120.0);
for _ in 0..120 {
    schedule.run(&mut world, step);
}

let position = world.get::<Position>(player).expect("the player is alive").0;
assert!((position[0] - 60.0).abs() < 1e-3);
```

This example is the crate's doc test, so it is compiled and run on every
`cargo test`. The full API is `mikan_ecs::{Entity, World, Schedule, DeadEntity}`
plus the iterator types returned by the queries.

The crate also ships a runnable smoke example — world, one entity, two systems,
sixty fixed steps, and a printed result:

```bash
cargo run -p mikan-ecs --example fixed_steps
```

## Entities

```rust
let entity = world.spawn();               // a fresh handle
assert!(world.is_alive(entity));          // resolves right now
world.despawn(entity);                    // frees the slot
assert!(!world.is_alive(entity));         // the handle is dead
let recycled = world.spawn();             // may reuse the slot index...
assert_ne!(entity, recycled);             // ...but never the generation
```

- Slots are reused in LIFO order, and the slot's generation starts at 1 and
  increments on every despawn. `Entity: Display` prints `{index}v{generation}`
  (`3v7`), so logs and assertions can name a specific handle.
- Every component operation on a dead handle fails or does nothing: `insert`
  returns `Err(DeadEntity { entity })`, `get`/`get_mut`/`remove`/`has` return
  `None`/`false`. No operation can silently address a different entity.
- `despawn` returns `true` the first time and `false` afterwards; despawning an
  entity removes **all** of its components from every storage.
- `entity_count` reports live entities. Slot indices are recycled, but handles
  never are: a long session can spawn unbounded entities with no ambiguity
  between old and new ones.

## Components and queries

| Call | Returns | Notes |
| --- | --- | --- |
| `insert(entity, component)` | `Result<(), DeadEntity>` | Inserts or replaces; a death check happens first |
| `remove::<T>(entity)` | `Option<T>` | Removes and returns the stored value |
| `get::<T>` / `get_mut::<T>` | `Option<&T>` / `Option<&mut T>` | Borrow tied to the world borrow |
| `has::<T>(entity)` | `bool` | |
| `iter::<T>()` | `Iterator<Item = (Entity, &T)>` | Insertion order |
| `iter_mut::<T>()` | `Iterator<Item = (Entity, &mut T)>` | Insertion order |
| `iter2::<A, B>()` | `Iterator<Item = (Entity, &A, &B)>` | Entities having both |
| `iter2_mut::<A, B>()` | `Iterator<Item = (Entity, &A, &mut B)>` | `A != B`, see below |
| `component_count::<T>()` | `usize` | Diagnostics (the demo logs it) |

Storage is one sparse set per component type, created on first insert. The dense
side of the set is a `Vec<(Entity, T)>` in insertion order, and removal keeps the
relative order of the remaining entries — so **iteration order is stable and
observable**: entities appear in the order their component was first inserted,
and removing one does not reshuffle the others.

That guarantee is why removal is O(n) in the number of stored components of that
type (the entries after the hole shift left, and their sparse indices are
updated). It is the trade this milestone chose: deterministic iteration is worth
more to gameplay code than removal throughput at the scales a "lite" ECS
targets. If a game ever despawns thousands of components per step, this is the
first line to revisit (see the consequences in ADR 0009).

### Two-component queries

`iter2_mut` yields the shared component of `A` next to the mutable component of
`B` for every entity that has both. The two storages are looked up with
`HashMap::get_disjoint_mut`, which the borrow checker proves disjoint — the
crate contains no `unsafe`. Because the proof only works for two *different*
types, `iter2_mut::<A, A>` panics with `"iter2_mut requires two different
component types"`. That is deliberate: the mistake is a programming error, and
it should be loud rather than aliasing.

The demo's `movement` system is the reference use:

```rust
for (_, position, motion) in world.iter2_mut::<Position, Motion>() {
    let distance = motion.speed * dt.as_secs_f32();
    position.0[0] += motion.direction[0] * distance;
    position.0[1] += motion.direction[1] * distance;
}
```

There are no filters and no three-way queries yet. A system that needs "all
entities with `A` and `B` but not `C`" iterates `iter2` and checks `has::<C>`
per entry; a system that needs three components runs two queries or is a
candidate for the next increment of the crate.

## Resources

```rust
world.insert_resource(MoveDirection([1.0, 0.0]));   // returns the previous value
let direction = world.get_resource::<MoveDirection>().map(|d| d.0);
*world.get_resource_mut::<Score>().unwrap() += 10;
world.remove_resource::<Score>();
```

Resources exist for the state a step needs but no entity owns. The established
pattern, used by the demo, is:

1. The application reads the outside world (input, viewport size) *before* the
   schedule runs.
2. It writes that state into resources (`MoveDirection`, `WorldBounds`).
3. Systems read resources and component storages and know nothing about the
   window, the input API, or the renderer — which is what keeps them
   unit-testable.

A step with no bounds resource still integrates but does not clamp; a missing
resource disables exactly the behavior that reads it. Systems must not panic on
missing resources they could treat as absent; the demo's `clamp` system
returns early instead.

## Systems and the schedule

```rust
let mut schedule = Schedule::new();
schedule.add("snapshot", ...).add("input", ...).add("movement", ...).add("clamp", ...);

schedule.run(&mut world, dt);      // every system once, in this order, with this dt
schedule.names();                  // ["snapshot", "input", "movement", "clamp"]
```

- The order systems were added in **is** the order they run. There is no
  dependency graph to reorder them and no parallelism to race them. When order
  matters, it is stated in one visible place — the call chain — and the names
  make that place readable in logs and tests.
- `run` executes every system exactly once. It has no timer of its own: the
  engine's `FixedTimestep` (in `mikan-core`) remains the only thing that decides
  how many simulation steps a frame performs. Running the schedule once per
  `Game::update` therefore cannot double-update the simulation, and
  `ctx.alpha()` interpolation keeps rendering smooth across frames that run
  zero steps.
- Systems are infallible (`FnMut(&mut World, Duration)`). Work that can fail
  belongs in an application hook, which can return an error; a system that needs
  to report something records it in a resource the caller inspects.
- Names are diagnostic labels: they need not be unique and do not affect
  execution. The demo logs `schedule.names()` at startup, and
  `scripts/verify_ecs.sh` asserts the order in that log.

## Driving it from the engine

`mikan` re-exports the crate (`mikan::World`, `mikan::Schedule`,
`mikan::Entity`, `mikan::DeadEntity`, and the `mikan_ecs` module). The demo's
`games/demo2d/src/player.rs` is the reference integration:

- `Game::init` spawns the player entity and inserts its components:
  `Position`, `PreviousPosition`, `Motion`, `Player`.
- `Game::update` writes the step's resources, then runs the schedule once. The
  platform already called this hook exactly once per fixed step.
- `Game::render` calls the pure `render_pose(world, entity, alpha)`, which
  interpolates between `PreviousPosition` and `Position`. Rendering never
  mutates the simulation.
- `Game::shutdown` logs `ecs: N entity, N fixed steps, N moved steps` — the
  moved-step count and the final position are what the end-to-end verification
  checks against each other.

## Determinism, honestly

Given the same binary, the same world, and the same `dt` sequence, a schedule
performs the same work in the same order — iteration and system order are both
specified, and no hash-map order is observable through the public API.
`crates/mikan-ecs/tests/determinism.rs` exercises this: repeating a step
sequence reproduces position and interpolation state **bit for bit**, and
despawn/reuse interleavings do not disturb the surviving order.

The limits, stated rather than implied:

- Floating-point results are not promised to be bit-identical across
  architectures, compilers, or optimization levels; the guarantee is for a
  given binary and target.
- Determinism is not a sandbox: systems can read resources they are not
  "supposed" to, and the crate cannot stop them. The demo keeps outside state
  out of systems by construction.

## Performance and limits

- Single-threaded by design. `World` makes no `Send`/`Sync` claim, and there is
  no parallel executor. One world per thread is the supported pattern.
- `iter2`/`iter2_mut` walk one dense storage and probe the other's sparse index
  per entry. For a handful of component types at gameplay scale this is well
  inside frame budgets; a profiled need for archetype-dense traversal would be
  the trigger to revisit storage, not a speculative rewrite.
- Removal is O(n) within one component type (order preservation). Insertion is
  amortized O(1). `component_count`, `entity_count`, and `is_alive` are O(1).
- No serialization, no change detection, no events, no filters, no command
  buffers. Nothing is stubbed; each arrives when a concrete need does.

## What this is not

- Not a general-purpose ECS, and not a scripting layer.
- Not physics, collision, or spatial querying — the roadmap's next milestone.
- Not a second scheduler: the fixed timestep stays where it already lives.
