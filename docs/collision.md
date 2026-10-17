# 2D collision

Mikan's 2D collision is axis-aligned boxes, a uniform-grid broad phase, and a
batch narrow-phase kernel written in C. This guide describes the crates, the
semantics, how the demo integrates them, and how the whole path is verified.
The decision record — including why the narrow phase is C and what that
boundary promises — is [ADR 0010](adr/0010-2d-collision-and-a-c-kernel.md).

## Crates and layers

| Crate | Role |
| --- | --- |
| `crates/collision-sys` | The native module: `src/mikan_contacts.h`, `src/contacts.c` (C99), a `build.rs` that compiles them with warnings-as-errors, the `#[repr(C)]` mirror types, and the raw `extern "C"` declaration. One of the three crates in the workspace allowed to use `unsafe`; the override and its reason are recorded in the crate root. |
| `crates/collision` | The safe face: `Aabb`, `Pair`, `Contact`, the uniform-grid `Broadphase`, the `reference` module, and `contacts` — one safe batch function. Contains no `unsafe`; `unsafe_code` stays `forbid`. |

Three layers, in call order:

1. **Broad phase (Rust).** `Broadphase::rebuild(&[Aabb])` sorts boxes into
   fixed-size cells (a box goes into every cell it touches); `pairs(&mut Vec)`
   reports every candidate pair that shares a cell — deduplicated and sorted,
   so hash-map order never reaches a caller. "Candidate" is exact: the grid
   can report pairs that do not overlap (they share a cell at a corner), and
   it can never miss one that does. Reuse one instance across steps; cells are
   emptied in place, so their allocations are kept.
2. **Narrow phase (C).** `contacts(&bounds, &pairs, &mut out)` is a single
   batch call into the C kernel, writing one `Contact` per pair, in pair
   order. The C side is one function over plain data: no allocation, no
   retained pointers, no state, no errors to translate.
3. **Resolution policy (the game's).** Which body actually moves is not the
   collision layer's decision. `mikan-collision` reports contacts; the demo's
   `collide` system applies them (below).

## Semantics

- An `Aabb` is `{ min: [f32; 2], max: [f32; 2] }`; `Aabb::from_center_size`
  builds one from a center and a full size. An empty box (`min == max`) is
  valid and never overlaps anything, not even itself.
- **Overlap means `depth > 0`.** Boxes that merely touch are *separated*, so a
  resting body does not jitter between resolved and touching.
- The `Contact` normal is a unit axis vector pointing from the pair's `first`
  box toward its `second`; translating `second` by `normal * depth` separates
  the pair. `depth` is `0.0` when separated.
- Non-finite coordinates make a pair separated, order-independently: the
  kernel never propagates NaN into a normal or a depth.
- The broad phase's skips match the kernel's view: degenerate, inverted, or
  non-finite boxes are never candidates because they can never overlap.

## The C kernel and its boundary

The ABI is one function and three plain structs (`mikan_aabb`, `mikan_pair`,
`mikan_contact`), all documented in `crates/collision-sys/src/mikan_contacts.h`:

```c
void mikan_contacts(const mikan_aabb *bounds, size_t bounds_len,
                    const mikan_pair *pairs, size_t pair_count,
                    mikan_contact *out);
```

`out` is caller-owned and sized by `pairs.len()`; the Rust wrapper asserts the
contract in debug builds and validates pair indices before the call, so a bad
index panics in Rust instead of reading out of bounds in C. A *raw* binding of
the same function is public in `mikan_collision_sys` (with `# Safety` docs)
purely so tests can exercise the C side's own validation.

Per `docs/ffi.md`, two things are recorded rather than implied:

- The module was adopted because the low-level numeric layer is C's assigned
  home in Mikan's architecture, **not** because it beat Rust; and
- the measurement is reported as measured. `scripts/bench_collision.sh` runs
  250,000 candidate pairs from 512 boxes through the C kernel and the Rust
  reference with the same inputs; the recorded result is **parity within
  run-to-run noise** (one run favors C, the next favors Rust). No speedup is
  claimed. The exact numbers live in `docs/STATUS.md`.

A Rust implementation of the same math is kept deliberately, in
`mikan_collision::reference`, as the test oracle and benchmark baseline. The
cross-check tests compare the two implementations bit-for-bit on fixed and
randomized cases, so drift between them fails loudly.

## The demo's integration

`games/demo2d/src/blocks.rs` binds collision to the ECS:

- A block is a `Position` + `Collider` entity; the player carries a `Collider`
  too. `spawn_blocks` returns the block entities so tests and callers can hold
  them.
- `CLASSIC_BLOCKS` is the shared layout: three blocks — a 140x140 pillar at
  `(700, 460)`, a 120x120 block at `(300, 460)`, and a 200x80 shelf at
  `(480, 120)`. All three stay clear of the y = 300 corridor the input and
  camera verifications walk along, and a unit test guards that.
- `collide_system()` returns the stateful `collide` system. Once per fixed
  step it rebuilds the broad phase from every `(Position, Collider)` entity in
  insertion order, computes contacts, and resolves each overlapping pair:
  a movable entity (one with `Motion`) is pushed out along the contact normal,
  half the depth each when both are movable, leaving anything without `Motion`
  in place. Its scratch buffers are reused across steps.
- The schedule is `snapshot, input, movement, collide, clamp`: collision runs
  after integration (so a body cannot end a step inside a block) and before
  the world-bounds clamp (so a resolution can never push a body out of the
  world). The order is stated once, in `player.rs`.

The demo logs `collision: 3 static blocks, grid cell 160` at startup;
`GRID_CELL` is 160 because the blocks are 120–200 px wide.

## Verification

- **Unit and cross-check tests** (`mikan-collision-sys`, `mikan-collision`):
  header semantics, boundary cases (touching, zero size, NaN), index
  validation, broad-phase determinism, and bit-exact agreement between the C
  kernel and the Rust reference — including a randomized 256-box case over
  thousands of steps.
- **`scripts/verify_collision.sh`** (Xvfb): the startup log reports the blocks
  and the grid; holding down+right drives the player into the first pillar's
  left face and *stops there* (x = 582 = face 630 minus the player's half
  width) while the keys slide it down to the floor clamp (y = 552) — passing
  through, stopping at first contact, or ejecting along the wrong axis all
  fail; and a straight run along y = 300 stays unobstructed.
- **`scripts/bench_collision.sh`** records the C-vs-Rust kernel numbers under
  `artifacts/bench/` (see `docs/STATUS.md` for the current results), and a
  Valgrind run of the benchmark binary is recorded there per the `ffi.md`
  checklist.

## What is not here

Rotation and circles (OBB or SAT), continuous collision, contact manifolds or
friction, layers and masks, and dynamic-vs-dynamic impulse resolution. Each is
a later increment; none is stubbed.
