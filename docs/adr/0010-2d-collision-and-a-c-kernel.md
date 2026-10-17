# 0010. 2D collision detection and resolution, with the narrow-phase kernel in C

- Status: Accepted
- Date: 2026-10-16

## Context

The roadmap's next milestone after ECS-lite is **2D collision detection and
resolution**: broad phase plus AABB resolution against the same fixed step.
Phase 5 also carries a project-owner directive that Mikan's stated architecture
— Rust + C++ forming the engine core, C as the low-level foundation — is a
requirement, not a long-term intention, and that this phase is where it starts
being real (the scripting half is ADR 0011). That collides head-on with ADR
0004, which deferred all C and C++ until a *measured* need cleared five
criteria in `docs/ffi.md`, including "the native replacement must beat [the
Rust implementation] by a margin".

This record resolves that tension honestly rather than silently:

- The driver here is the architecture directive, **not a profiled bottleneck**.
  AABB overlap math is a handful of float comparisons; the honest expectation
  is near-parity with Rust, and the measurement section below is written to
  report parity as parity. No speedup is claimed, and the C kernel is not
  adopted because it is faster; it is adopted because the low-level numeric
  layer of this engine is C's assigned home, and the point of the first native
  module is to establish a small, audited, well-tested boundary.
- ADR 0004 remains the policy for modules proposed *on performance grounds*.
  For those, its criteria stand unmodified. This record supersedes it only for
  this module, and states exactly what ffi.md demands: the need, the boundary
  design, the measurement plan, and the verification plan.

Open questions the milestone had to answer:

1. **Where does the Rust/C line sit?** Whole-system in C (broadphase state,
  arena, sort) would duplicate a subsystem for no measured reason. Whole-system
  in Rust would leave C decorative. The narrow phase — pure, stateless,
  branch-light math over plain structs — is the natural C kernel; broad phase,
  storage, ECS integration, and policy stay in Rust.
2. **What shape is the boundary?** `ffi.md` forbids crossing per object per
  frame and demands slices over element-by-element calls. The kernel is one
  batch call per collision step: candidate pairs in, contacts out.
3. **Touching versus overlapping.** If depth 0 counted as a contact, a resting
  body would jitter (resolved, then touching again). Definition: `depth > 0`
  is an overlap; touching is separated.
4. **How does resolution compose with the existing schedule?** The classic
  scene already clamps to the world bounds. Collision inserts a step between
  `movement` and `clamp`: `snapshot, input, movement, collide, clamp`, so the
  final state is always inside the world and the observable behavior of
  existing scenes is preserved.
5. **What stays out?** Rotation, circles, continuous collision, dynamic-vs-
  dynamic impulse splitting, layers/masks, and spatial-query API beyond the
  broadphase's own pairs. Each is a later increment; nothing is stubbed.

## Decision

1. **Two new crates, following the required structure in `docs/ffi.md`:**
   - `crates/collision-sys` — the native module: C99 sources, a
     `build.rs` that compiles them with the `cc` crate (warnings-as-errors for
     the module's own sources), the `#[repr(C)]` mirror structs, and the raw
     `extern "C"` declarations. `unsafe` is allowed in this crate only, every
     occurrence carries a `// SAFETY:` comment, and the reason is recorded in
     the crate root.
   - `crates/collision` — the safe wrapper and everything else: `Aabb`,
     `Contact`, the uniform-grid `Broadphase`, resolution helpers, and the
     `contacts` entry point that calls the kernel. It contains **no unsafe**;
     the workspace `unsafe_code = "forbid"` lint applies unchanged.
2. **The C surface is one function.** Everything crosses in a single batch
   call per step:

   ```c
   void mikan_contacts(const mikan_aabb *bounds, size_t bounds_len,
                       const mikan_pair *pairs, size_t pair_count,
                       mikan_contact *out);
   ```

   Plain data only: two float-vector structs, an index pair, and an out
   buffer sized by the caller. No pointers retained, no allocation, no state,
   no errors to translate, no thread-affinity concerns beyond "pure function".
   Semantics are documented in the header and mirrored in the wrapper: for
   pair `(i, j)`, the contact's normal points from `i` toward `j` with
   `depth > 0` when the boxes overlap, and `depth <= 0` means separated
   (touching is separated).
3. **A Rust reference implementation is kept — deliberately.** The same
   math lives in `mikan_collision::reference` (a few dozen lines, no
   abstraction). It is used by the test suite as the correctness oracle
   (the C kernel's results are cross-checked against it, including boundary
   and randomized cases) and by the benchmark as the baseline. That is a
   concrete requirement of this milestone, not duplication for its own sake,
   and the module says so in its docs.
4. **Broad phase and integration are Rust.** `Broadphase` is a uniform grid
   over slices of AABBs with deduplicated candidate pairs; the demo binds
   `Collider` components to entities, rebuilds the grid from
   `(Position, Collider)` each `collide` step in insertion order, resolves
   movable entities (those with `Motion`) against static ones along the
   contact normal, and leaves everything else to the existing `clamp` step.
   Ordering and limits are documented in `docs/collision.md`.
5. **The native module is "done" only per `docs/ffi.md`'s checklist**: tests
   through the wrapper exercise the kernel including boundary cases; the C
   sources build with warnings-as-errors; a doc test and an integration
   example exist; `docs/ARCHITECTURE.md` places the module in the dependency
   graph; and a sanitizer or Valgrind run is recorded in `docs/STATUS.md`.
6. **The measurement is reported as measured.** `scripts/bench_collision.sh`
   runs the same pair sets through the C kernel and the Rust reference with
   the repository's benchmark protocol (release, fixed inputs, recorded
   conditions) and records the numbers — whatever they are — in
   `docs/STATUS.md`. Criterion 3 of ADR 0004 is *not* claimed; the record
   says that explicitly.

## Consequences

Positive:

- The engine gets its first native module with the smallest boundary this
  milestone could justify: one pure function, plain data, no lifetime or
  ownership rules to re-express — and it is verified against an independent
  implementation rather than trusted.
- Broad phase, ECS integration, and policy remain ordinary, testable Rust;
  nothing about the collision *system* depends on the C kernel being present
  conceptually, even though the production path calls it.
- The demo's existing behavior is preserved: the world-bounds clamp still runs
  last, and the classic scene's scripts (input, camera, pixel-perfect) keep
  their expected numbers.
- The boundary rules that later modules must follow now have a worked
  example in the repository.

Negative:

- **A C compiler is now a build prerequisite** on every platform; `ffi.md`,
  `DEVELOPMENT.md`, and CI must say so. The "no C/C++ toolchain required"
  claim is no longer true and is updated rather than quietly left stale.
- `unsafe` now exists in first-party code (exactly one crate, documented).
  The workspace lint stays `forbid` everywhere else.
- The kernels are expected to measure at parity with Rust; the C layer's
  value is architectural (the assigned home of low-level numerics), and the
  status record says so instead of implying a speedup.
- Two implementations of the contact math must stay in sync; the cross-check
  tests fail loudly if they drift, which is the point.
- Collision behavior is axis-aligned only; rotated colliders need oriented
  boxes or circles, which are future work, not half-built now.
