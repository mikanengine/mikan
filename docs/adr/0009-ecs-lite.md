# 0009. ECS-lite: generational entities, sparse-set components, and scheduled systems

- Status: Accepted
- Date: 2026-10-16

## Context

The roadmap's next milestone adds entity storage and systems, scheduled on the
fixed timestep the engine already runs ("ECS-lite"). Several decisions inside
that milestone are genuinely open:

1. **Use an existing ECS?** A mature crate (`bevy_ecs`, `hecs`) would bring
   archetype storage, query caching, a parallel executor, and change detection
   on day one. The milestone asks for none of those: it asks for entities,
   components, and a schedule a reader can follow. The engine's dependency
   budget is part of its value ("the engine's value is partly in what it does
   not pull in" — `DEVELOPMENT.md`), and an external ECS would tie the engine's
   public vocabulary to another project's release cadence and data model.
2. **How do entity handles stay valid?** A bare index goes stale the moment the
   slot is reused: a dangling handle silently addresses a different entity.
   Something must make that impossible to ignore.
3. **What does iteration order guarantee?** Systems iterate components in
   batches, and the order in which they see entities is observable behavior,
   not an implementation detail: it feeds positions, matchmaking, and the
   verification logs. Swap-removal is the cheapest deletion and the most
   unstable iteration; the choice between them has to be made and stated.
4. **How do two-component queries work without aliasing?** `for (position,
   velocity) in ...` must hand out `&mut Position` and `&mut Velocity` for the
   same entity, which safe Rust cannot express through one map lookup at a
   time. The query shape decides how much of the borrow checker's work moves
   into the crate.
5. **Where does scheduling stop?** A dependency graph, a parallel executor, a
   fixed-timestep accumulator of its own — each is a large subsystem. The
   engine already has exactly one thing that decides when and how many
   simulation steps run (`mikan-core::FixedTimestep`). A second scheduler would
   be two schedulers.
6. **What does "deterministic" mean here?** The claim is only useful if it is
   precise: same world, same systems, same `dt` sequence, same order — and
   tested rather than asserted, with the floating-point caveat written down.
7. **Does this milestone add C or C++?** The same question ADRs 0006, 0007, and
   0008 answered for their milestones. The ADR 0004 criteria — a measured need
   clearing all five requirements in `docs/ffi.md` — again have no measurement
   behind them: this subsystem is table bookkeeping, where the feature is
   precisely the safety Rust enforces at compile time (handle validity,
   aliasing, lifetime-tied iteration). A C/C++ boundary here would be wide
   *and* would trade away the property being built. There is no native
   implementation to measure against because there is no reason to propose
   one.

## Decision

1. **A new CPU-only crate: `mikan-ecs`.** It depends on `thiserror` only — no
   workspace crate, no ECS backend, no threading library — and knows nothing
   about windows, graphics, or the engine's runtime. `crates/mikan` re-exports
   it (the crate module and `DeadEntity`, `Entity`, `Schedule`, `World`), and
   the application drives it.
2. **Entities are generational handles: `Entity { index, generation }`.**
   Spawning reuses free slots in LIFO order and increments the slot's
   generation, which starts at 1 and is never reset. A despawned handle can
   therefore never resolve, not even after its slot is reused; `Display`
   prints `{index}v{generation}` so logs distinguish handle identities. Every
   component operation checks the full handle, generation included.
3. **Components live in one sparse set per type, keyed by `TypeId`.** A
   `HashMap<TypeId, Box<dyn ErasedStorage>>` owns all storages; each storage is
   a `Vec<Option<u32>>` sparse index over a dense `Vec<(Entity, T)>` of
   insertion-ordered entries. Removal is **order-preserving** (`Vec::remove`
   plus a sparse-index fixup) instead of `swap_remove`: iteration always visits
   entries in the order they were inserted, and removal keeps the relative
   order of the rest. The cost — an O(n) shift per removed entry — is stated,
   not hidden; it is the trade this milestone chooses for deterministic
   iteration. Despawning removes the entity from every storage.
4. **Resources are untyped singletons per world.** `insert_resource` returns
   the previous value, `get`/`get_mut`/`remove_resource` return options. They
   exist for the state a *step* needs but no entity owns — the demo writes the
   input direction and the world bounds before the schedule runs, so systems
   never touch the window or the input API.
5. **Queries are explicit and minimal.** `iter`/`iter_mut` for one type;
   `iter2`/`iter2_mut` for two, yielding the shared component of `A` next to
   the (mutable) component of `B`. `iter2_mut` requires two *different*
   component types: it asserts this (`"iter2_mut requires two different
   component types"`) and uses `HashMap::get_disjoint_mut` so the borrow checker
   — not an unsafe block — proves the two storages are disjoint. There are no
   filters, no query caching, and no three-component queries in this milestone.
6. **`Schedule` runs named systems once each, in registration order**, with the
   `dt` the caller passes — nothing else. No dependency graph, no parallel
   executor, no per-system timers, no `Send` requirement. `names()` reports the
   registration order (the demo logs it, and the verification script greps the
   log). The engine integration is therefore one line of intent: an application
   runs `schedule.run(&mut world, dt)` once per `Game::update`, and the
   platform's existing `FixedTimestep` keeps deciding how many updates a frame
   performs and what `alpha` the renderer interpolates with.
7. **Determinism is a tested claim.** The same schedule run over the same world
   with the same `dt` sequence performs the same work in the same order: no
   hash-map iteration reaches the public surface, storages iterate dense arrays,
   and systems run in registration order. `tests/determinism.rs` reproduces
   results bit-for-bit across runs in one process; the honest limits
   (floating-point results may differ across targets or after recompilation;
   nothing promises reproducibility across machines) are documented on
   `Schedule::run` and in `docs/ecs.md`.
8. **The demo migrates: the classic scene's player is an entity.** Components
   `Position`, `PreviousPosition`, `Motion`, and `Player`; resources
   `MoveDirection` and `WorldBounds`; systems `snapshot`, `input`, `movement`,
   `clamp` in exactly that order (snapshot before movement is what makes
   interpolation correct; clamp after movement is what keeps the player inside
   the world for every observable state). The per-step arithmetic is kept
   bit-for-bit (`distance = speed * dt`, then `direction * distance`), and
   rendering is pure: `render_pose` interpolates between the last two fixed
   states and never mutates the simulation. `games/demo2d/src/square.rs` is
   replaced by `games/demo2d/src/player.rs`.
9. **No native module in this milestone.** The ADR 0004 criteria remain unmet;
   this record exists so the answer to "why is there still no C or C++?" is an
   accepted decision — the subsystem's value is compile-time-enforced safety
   over a wide, allocation-heavy data structure, not a narrow native boundary —
   rather than an omission.

## Consequences

Positive:

- A stale or double-despawned handle is an `Option`/error (`DeadEntity`), never
  a silent alias for a different entity; slot reuse cannot resurrect old
  handles.
- Iteration order is insertion order, everywhere and by construction; combined
  with registration-order systems, simulation behavior is reproducible for a
  given binary, which the determinism tests exercise in bits (40 unit tests, 5
  integration tests, 4 doc tests).
- The crate is testable without a window, a GPU, or the engine; the demo's
  systems are unit-tested against the real `World` and `Schedule`.
- One new dependency (`thiserror`, already in the workspace through
  `mikan-core`), no `unsafe` (the workspace lint stays intact), and no second
  build system or toolchain.
- The schedule's name list is observable, so the demo log and
  `scripts/verify_ecs.sh` can assert the *order* of systems, not just their
  effects.

Negative:

- Order-preserving removal is O(n) in the dense vector per removed component;
  a game despawning thousands of entities per frame would notice. Accepted for
  the scales this milestone targets and documented in `docs/ecs.md`; the
  alternative (`swap_remove`) trades iteration determinism for removal speed.
- No archetypes: `iter2` walks one storage and probes the other per entry, so a
  query over two small components out of many pays sparse-set probes rather
  than cache-dense archetype chunks. Measured only by the tests' scale; a
  profiled need would be the trigger to revisit.
- The world is single-threaded by design; there is no `Send`/`Sync` guarantee
  and no parallel executor. Gameplay code cannot share a `World` across
  threads, and this is stated rather than worked around.
- `iter2_mut` panics on two identical component types — an explicit assertion
  (the mistake is a programming error, and it should be loud) instead of the
  aliasing UB that a transparent unchecked path would allow.
- No change detection, no events, no query filters, no serialization, no
  command buffers. Nothing is stubbed; each is a local addition when a
  concrete need arrives.
