# 0012. Scene and entity transform hierarchy, with a C compose kernel and a hand-written C++ scene graph

- Status: Accepted
- Date: 2026-10-17

## Context

The milestone after collision and scripting is the **scene and entity system**:
the layer that answers where each entity is, what its parent is, and which
entity updates run in which order. It sits between the ECS (`mikan-ecs`) and
the frame loop, and it is the first subsystem whose data structure is a
*dynamic graph* rather than an array of plain structs.

Phase 5 established the project-owner directive that Mikan's stated
architecture — Rust + C++ forming the engine core, C as the low-level
foundation — is a requirement, not a long-term intention. Collision gave C a
home (ADR 0010) and scripting gave C++ a home through the vendored Luau VM
(ADR 0011), but the Luau C++ is **third-party**: Mikan writes no C++ glue there.
This milestone is where Mikan writes C++ of its own, and where a native module
is proposed on architectural rather than performance grounds for the third time.

As ADR 0010 did, this record resolves the tension with ADR 0004 honestly:

- The driver is **the architecture directive, not a profiled bottleneck**. A
  hierarchy over a few dozen entities is not a bottleneck; the honest
  expectation is that a native graph with real bookkeeping is *slower* than a
  Rust array fold, and the measurement below reports that, not a speedup.
- ADR 0004 remains the policy for modules proposed *on performance grounds*;
  for those its criteria stand unmodified. This record supersedes it only for
  this module, and states what `docs/ffi.md` demands: the need, the boundary
  design, the measurement plan, and the verification plan.

Open questions the milestone had to answer:

1. **Where does the Rust/C/C++ line sit?** Three honest options were on the
   table: all-Rust (leaves the C++-core claim decorative for a third
   consecutive milestone), all-native (the ECS, the schedule, and the scene
   policy belong to Rust), and split. The split chosen follows the data:
   **C++ owns the dynamic graph** (topology, cycle rejection, dependency order,
   children lists — the part that is genuinely a pointer-heavy data structure),
   **C owns the numeric composition** (pure `f32` multiply/add over a plain
   `#[repr(C)]` struct, exactly the kind of kernel C already owns), and **Rust
   owns everything else** (the ECS integration, scenes, deferred despawn,
   manager, policy). This is the first module with hand-written C++ *and*
   hand-written C in one boundary.
2. **What shape is the boundary?** `ffi.md` forbids C++ classes, exceptions,
   templates, or STL types crossing the boundary and forbids crossing per
   object per frame. The graph is one **opaque handle** (`mikan_scene_graph`)
   allocated and freed by the caller; every entry point is `extern "C"`, takes
   plain data plus lengths, and returns a status code. Composition crosses as
   one **batch call** (`mikan_scene_graph_propagate`): locals in, world
   transforms out, one call per scene per step. Transforms are a seven-`f32`
   `#[repr(C)]` struct read and written by value; no pointer is retained.
3. **How is composition defined exactly?** Standard 2D TRS
   (`out = parent ∘ local`): the child's translation is scaled and rotated by
   the parent, then the parent's translation is added; rotations sum; scales
   multiply. Rotation is the source of truth, and the struct also carries the
   cached unit pair `(cos, sin)` so the kernel **never computes trigonometry**:
   Rust caches the pair once per local transform, and the kernel keeps it exact
   through the addition identities. Every product/sum has a fixed grouping,
   mirrored operation-for-operation by an independent Rust reference, so the
   two agree **bit for bit** (compared through `to_bits`, not `==`).
4. **What are the hierarchy invariants?** It is a forest: every node has at
   most one parent, node id `0` is the reserved "no parent" sentinel, a link
   that would close a cycle is rejected without mutating anything, and removing
   a node **re-roots its children** (they keep their local transforms) so no
   dangling edge can outlive an entity. Insertion order is stable and is the
   shared index space for `nodes` and `propagate`.
5. **Where does the scene update order come from?** `Scene::update` runs the
   systems, flushes deferred despawns, then propagates transforms. Because
   propagation is last, a system that reads a `GlobalTransform2d` sees the
   previous step's world (a documented one-step lag); readers outside the
   schedule always see the current step. `GlobalTransform2d` is a cache rebuilt
   from scratch, never simulation state.
6. **What stays out?** World-space-preserving reparent bakes, scene
   serialization, multiple active scenes, per-scene input focus, and any
   parallel traversal. Each is a later increment; none is stubbed.

## Decision

1. **Two new crates, following the required structure in `docs/ffi.md`:**
   - `crates/scene-sys` — the native module: `src/transform.c` (C, the compose
     kernel), `src/scene_graph.cpp` (C++, the topology), the two headers, a
     `build.rs` that compiles each with warnings-as-errors (the C file on the C
     driver, the C++ file on the C++ driver with exceptions/RTTI disabled, so
     C-only and C++-only flags never mix), the `#[repr(C)]` transform mirror,
     the status codes, and the raw `extern "C"` declarations. `unsafe` is
     allowed in this crate only, every occurrence carries a `// SAFETY:`
     comment, and the reason is recorded in the crate root.
   - `crates/scene` — the safe wrapper and everything else: `Transform2d`,
     `GlobalTransform2d`, `Hierarchy`, `DespawnQueue`, `Scene`, `SceneManager`,
     and `reference` (the independent Rust compose). It contains **no unsafe**.
2. **The C surface is one function, the C++ surface is one opaque type.** The
   C kernel:
   ```c
   void mikan_transform2d_compose(const mikan_transform2d *parent,
                                  const mikan_transform2d *local,
                                  mikan_transform2d *out);
   ```
   and the C++ graph (`mikan_scene_graph_create/destroy`, `len`, `contains`,
   `add`, `remove`, `set_parent`, `clear_parent`, `parent`, `children`,
   `nodes`, `propagate`), all `extern "C"`, all plain data in and status codes
   out. The graph stores only topology and identity (opaque `u64` handles that
   Rust packs from `Entity::to_bits`); it composes nothing itself — `propagate`
   walks the nodes parents-first and delegates every arithmetic operation to
   the C kernel. Allocation failure is fatal (the module builds without
   exception support); nothing throws across the boundary.
3. **A Rust reference implementation is kept — deliberately.** The same compose
   math lives in `mikan_scene::reference` (a dozen lines). It is the test
   oracle (the C kernel's results are cross-checked against it, boundary and
   randomized, bit for bit) and the benchmark baseline. That is a requirement
   of this milestone, not duplication for its own sake, and the module says so.
4. **The safe wrapper adds the engine semantics.** `Hierarchy` maps the native
   status codes to `HierarchyError::{UnknownEntity, AlreadyPresent, Cycle}`;
   `Scene` owns one `World`, one `Schedule`, and the update order, and stores
   the hierarchy as a world resource so systems can query parent links;
   `flush_despawns` is the deferred-destruction path for systems; and
   `propagate_transforms` prunes dead nodes, reconciles entities that gained a
   `Transform2d`, composes in one batch call, and writes one `GlobalTransform2d`
   per node. With no `Hierarchy` resource the function is a no-op, so a bare ECS
   world pays nothing.
5. **The native module is "done" only per `docs/ffi.md`'s checklist**: tests
   through the wrapper exercise both the C and C++ sides including error paths
   and the removal-re-roots-children invariant; the sources build with
   warnings-as-errors; doc tests and an integration example exist;
   `docs/ARCHITECTURE.md` places the module in the dependency graph; and a
   Valgrind run is recorded in `docs/STATUS.md`.
6. **The measurement is reported as measured.** `scripts/bench_scene.sh` runs
   the same hierarchy and local transforms through the native graph and through
   a Rust reference fold in release, and records the numbers — whatever they
   are — in `docs/STATUS.md`. Criterion 3 of ADR 0004 is *not* claimed.
7. **The demo demonstrates the whole path.** `--scene hierarchy` builds a sun,
   three planets, and a moon as entities (`hierarchy.rs`): the planets are
   children of the sun and the moon is a child of the first planet. `A`/`D`
   rotate the sun, so the planets and the moon orbit rigidly; `F` despawns the
   moon through the deferred queue. The classic scene draws blocks and player
   through the renderer's sprite collector, keeping every existing verification
   number unchanged.

## Consequences

Positive:

- The engine gets its first **hand-written C++** in a boundary built exactly the
  way `ffi.md` prescribes (opaque handle, `extern "C"` only, plain data,
  no exceptions or RTTI), next to a C numeric kernel — the two-language split
  the architecture claims, in one module.
- The graph's invariants (forest, no cycles, re-root on removal, stable order)
  are enforced once, in one place, and cross-checked against an independent Rust
  implementation rather than trusted.
- The ECS, scenes, the manager, and despawn policy remain ordinary, testable
  Rust; the scene layer is usable with or without transform hierarchy.
- The demo gains a visible, verifiable hierarchy: rigid orbital rotation and a
  deferred despawn that the capture checks prove frame by frame.

Negative:

- **The native graph is materially slower than a Rust array fold.** The
  recorded benchmark (see `docs/STATUS.md`) shows the native propagation
  spending an order of magnitude more time per node (about fifteen times in the
  recorded run) than a Rust fold with a
  precomputed parent array, because it carries real graph bookkeeping
  (`unordered_map` index and parent lookups, per-call traversal buffers) that
  the reference gets for free. This is reported, not hidden: the driver was
  architectural, the arithmetic kernel is at parity, and no speedup is claimed.
  Caching the index/topology and moving to a `Vec`-backed node table are future
  work if a scene ever becomes hot.
- A C++ compiler is now needed for a second crate (scene-sys), and the module
  needs both a C and a C++ compiler even though the rest of the build already
  required them.
- `unsafe` now exists in two crates (`mikan-collision-sys` and
  `mikan-scene-sys`); the workspace lint stays `forbid` everywhere else.
- Two implementations of the compose math must stay in sync; the cross-check
  tests fail loudly if they drift, which is the point.
- Deferred despawn and one-step-lagged world transforms are deliberate trades
  that callers must understand; both are documented in `docs/scene.md`.
