# Scenes and the entity transform hierarchy

Mikan's scene system is the layer between the ECS (`mikan-ecs`) and the frame
loop. It answers four questions: where each entity is (transforms), what its
parent is (the hierarchy), what a scene is (a world, a schedule, and a fixed
update order), and how scenes coexist (a manager with one active scene). The
hierarchy's topology is a hand-written C++ graph and its transform composition
is a C kernel; everything else is safe Rust. The decision record — including
why the native layer exists and its honest measurement — is
[ADR 0012](adr/0012-scene-and-entity-system.md).

## Crates and layers

| Crate | Role |
| --- | --- |
| `crates/scene-sys` | The native module: `src/transform.c` (C, `mikan_transform2d_compose`), `src/scene_graph.cpp` (C++, the topology), `src/mikan_transform.h` and `src/mikan_scene_graph.h` (the contract), a `build.rs` that compiles them with warnings-as-errors and `-fno-exceptions -fno-rtti`, the `#[repr(C)]` transform mirror and status codes, and the raw `extern "C"` declarations. One of the three crates allowed to use `unsafe`; the override and its reason are recorded in the crate root. |
| `crates/scene` | The safe face: `Transform2d`, `GlobalTransform2d`, `Hierarchy`, `DespawnQueue`, `Scene`, `SceneManager`, and `reference`. Contains no `unsafe`. |

Three layers, in call order:

1. **Topology (C++).** The scene graph stores who is the parent of whom. It is
   a forest over opaque `u64` node ids; it holds no transforms and computes no
   arithmetic. `mikan_scene_graph_propagate` walks the nodes parents-first and
   delegates composition to the C kernel.
2. **Composition (C).** `mikan_transform2d_compose` composes a parent world
   transform with a local one (`out = parent ∘ local`) over a plain
   `#[repr(C)]` seven-`f32` struct. One batch call per scene per step.
3. **Scene semantics (Rust).** `Scene` owns the world and schedule, orders the
   update, and stores the hierarchy as a resource; `Hierarchy` wraps the opaque
   graph in a safe RAII type and translates status codes into errors.

## Transforms

- `Transform2d { translation: [f32; 2], rotation: f32, scale: [f32; 2] }` is
  **simulation state**: systems mutate it, and the engine never does. Rotation
  is in radians.
- `GlobalTransform2d` is the derived world-space result. `propagate_transforms`
  rebuilds it from scratch every step; writing to it directly has no lasting
  effect. It keeps the composed `(cos, sin)` pair, so reading a world rotation
  never re-rounds through trigonometry.
- Composition is standard 2D TRS: the child's translation is scaled and rotated
  by the parent, then the parent's translation is added; rotations sum; scales
  multiply. Scale follows the arithmetic — a zero or negative scale produces a
  collapsed or mirrored world transform, with no required normalization.
- Non-finite inputs are not special-cased: a transform has no "separated"
  answer, so the kernel computes IEEE arithmetic and a non-finite input yields a
  non-finite output. Gameplay code that cares must validate before composing.
- **Rotation is exact through composition.** The source of truth is the radian
  sum; the kernel never calls `sin`/`cos`. Rust caches the unit pair once per
  local transform, and the kernel keeps it exact with the addition identities.
  This is why the C result is bit-identical to the Rust reference (below).

## Hierarchy invariants

`Hierarchy` is a forest over `Entity` handles (packed with `Entity::to_bits`,
so a stale handle can never alias a reused slot):

- every node has at most one parent; a node with no parent is a root;
- node id `0` is reserved as the "no parent" sentinel and is never a valid node;
- a link that would make a node its own ancestor is rejected with
  `HierarchyError::Cycle`, **without changing the graph**;
- removing a node **re-roots its children** (they keep their local transforms
  and their relative order), so no dangling edge can outlive an entity;
- `children` follows attachment order; `entities` follows insertion order, and
  that order is the shared index space for propagation.

Topology changes never rewrite transforms. A child that loses its parent simply
becomes a root at the same local transform. Reparenting that preserves world
placement is a gameplay operation (read `GlobalTransform2d`, compute and write
the new local), not a hidden side effect.

## A scene and its update order

`Scene::new(name)` creates one `World` and one `Schedule` and inserts an empty
`Hierarchy` resource. `Scene::update(dt)` does exactly three things, in order:

1. **Systems** — `schedule.run(world, dt)`, once each, in registration order.
2. **Deferred despawns** — `flush_despawns` applies every `request_despawn`
   made during the step.
3. **Transform propagation** — `propagate_transforms` prunes dead nodes,
   reconciles entities that gained a `Transform2d`, composes world transforms
   parents-first in one native batch call, and writes one `GlobalTransform2d`
   per node.

The order is the contract. Because propagation runs **after** the systems, a
system that reads `GlobalTransform2d` sees the worlds of the *previous* step (a
one-step lag for entities it just moved); readers outside the schedule — the
renderer, tests, queries after `update` — always see the current step.
`Scene::spawn` registers a node immediately (so a fresh entity can be a parent
before the next update); entities spawned through `world_mut().spawn()` are
reconciled as roots by the next propagation.

`propagate_transforms` is a free function over `&mut World`, and it is a no-op
when the world has no `Hierarchy` resource — a bare ECS world pays nothing.
An entity whose `Transform2d` was removed while it stayed alive contributes the
identity local transform; it keeps a derived `GlobalTransform2d` (the cache
reports where the entity *is*, and with no local transform that is the parent's
frame).

## Deferred despawn

Systems iterate through borrows, so a system cannot despawn an entity in the
middle of its own loop. `request_despawn(world, entity)` records the entity in
a `DespawnQueue` resource; `flush_despawns` later removes each live request from
the hierarchy (re-rooting its children) and then despawns it from the world, in
request order. Stale requests are skipped, and despawns are idempotent. A
renderer that collects components through the world therefore cannot see a
destroyed entity, whether it died immediately or through the queue.

`Scene::despawn` is the immediate form (outside systems); `Scene::despawn_later`
is the deferred form (anywhere).

## The scene manager

`SceneManager` holds scenes in slots, hands out generational `SceneId`s
(`{index}v{generation}`, like entity handles), and runs exactly one **active**
scene per `update`. Switching is an id operation: a deactivated scene keeps its
world, systems, and state until it is activated again or removed. Removing a
scene deactivates it and invalidates its id even after the slot is reused.
`replace` swaps in a freshly built scene while keeping the id and its active
state — a cheap restart.

## The native boundary and its measurement

The C ABI is one function over one plain struct; the C++ ABI is one opaque
handle over plain data and status codes, all documented in
`crates/scene-sys/src/mikan_transform.h` and
`crates/scene-sys/src/mikan_scene_graph.h`:

```c
void mikan_transform2d_compose(const mikan_transform2d *parent,
                               const mikan_transform2d *local,
                               mikan_transform2d *out);
```

The graph is created and destroyed by the caller (the safe `Graph`/`Hierarchy`
owns it and frees it exactly once in `Drop`), is not thread-safe (encoded as
`&mut self`), and stores entity identities as opaque `u64` handles so it stays
free of ECS types. Every entry point validates its arguments and returns a
status code; null pointers are tolerated and reported, never crashed.

Per `docs/ffi.md`, two things are recorded rather than implied:

- The module was adopted because the low-level numeric kernel is C's assigned
  home and a dynamic graph is C++'s, **not** because it beat Rust; and
- the measurement is reported as measured. `scripts/bench_scene.sh` runs the
  same hierarchy and local transforms through the native graph and through a
  Rust reference fold in release. **The native graph is markedly slower** — it
  carries real graph bookkeeping (`unordered_map` index and parent lookups and
  per-call traversal buffers) that the reference gets for free — and no speedup
  is claimed. The exact numbers live in `docs/STATUS.md`.

A Rust implementation of the compose math is kept deliberately, in
`mikan_scene::reference`, as the test oracle and benchmark baseline. The
cross-check tests compare the two implementations bit for bit (through
`to_bits`, which distinguishes `-0.0` from `0.0`), including a large randomized
hierarchy, so drift between them fails loudly.

## The demo's integration

`games/demo2d/src/hierarchy.rs` binds the scene system to the ECS and the
renderer:

- `spawn(scene, player_uv, marker_uv)` builds a sun (a root), `PLANET_COUNT`
  planets (children of the sun), and a moon (a child of the first planet). Each
  body carries a `Transform2d` and a `SpriteRenderer` (the shared sprite type in
  `renderable.rs`).
- `Spin` is a resource; the `spin` system rotates the sun by `SPIN_SPEED` per
  second in the direction of the held key, and propagation carries the planets
  and moon around it rigidly.
- `F` calls `scene.despawn_later(moon)`; the next update's flush removes the
  moon and its node, re-rooting nothing (it is a leaf).
- The startup log reports the bodies and the system:
  `hierarchy: sun <e> + [<planets>] + moon <e> | orbit 160, moon orbit 80 | A/D spin, F despawn`.

The classic scene now draws its blocks and player through
`renderable::collect`, which walks `(SpriteRenderer, GlobalTransform2d)` in
insertion order and writes sprites in painter order. The hierarchy scene shares
that collector; with no camera the world positions are the pixels the capture
checks probe.

## Verification

- **Unit and cross-check tests** (`mikan-scene-sys`, `mikan-scene`): kernel
  semantics and error paths, forest invariants (duplicates, cycles, unknown
  entities, re-root on removal), propagate ordering, deferred despawn, the
  manager's generational ids and exclusive activation, and bit-exact agreement
  between the C kernel and the Rust reference on a randomized hierarchy.
- **`scripts/verify_scene.sh`** (Xvfb): with no input, the sun is warm at
  `[480, 300]`, the first planet is blue at `[640, 300]`, and the moon is a
  yellow star at `[720, 300]`; holding `D` spins the sun so the star's centroid
  stays exactly one orbit radius (240 px) from the sun while its angle changes
  by more than 5° (rigid rotation); and pressing `F` removes the star pixels
  while the sun and the planets remain.
- **`scripts/bench_scene.sh`** records the native-versus-Rust numbers under
  `artifacts/bench/` (see `docs/STATUS.md`), and a Valgrind run of the benchmark
  binary is recorded there per the `ffi.md` checklist.

## What is not here

World-space-preserving reparent bakes, scene serialization or prefabs, multiple
simultaneously active scenes, per-scene input routing, parallel or
cache-friendly traversal, and any spatial query beyond parent/children lookup.
Each is a later increment; none is stubbed. `GlobalTransform2d` is
world-space only — there is no `LocalTransform`/`Transform` split, no 3D, and
no camera integration beyond what the demo does.
