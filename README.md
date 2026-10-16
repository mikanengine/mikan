# mikan

Mikan 蜜柑 — a small, modular game engine. 2D first, then 3D. Rust at the core, C at the foundation, C++ where it counts.

> 小さな房が、世界になる。
> Small segments, whole worlds.

## What works today

This is an early but real vertical slice: a windowed application that renders at
vsync, loads PNG sprites and sprite-sheet metadata from disk, draws textured
(rotated, tinted, alpha-blended) sprites with per-sprite sampling, through a
minimal 2D camera with an optional pixel-perfect mode, reads the keyboard, runs
a fixed-timestep simulation whose classic-scene player is an ECS entity driven
by explicitly ordered systems and a Luau gameplay script, resolves AABB
collisions against the scene's static blocks, handles resize and every
recoverable swap-chain condition, reports draw statistics, and exits cleanly.

| Capability | State |
| --- | --- |
| Window + event loop (winit) | implemented, runtime-verified |
| Fixed-timestep simulation with interpolation | implemented, unit-tested |
| Keyboard/mouse state with press/release edges | implemented, unit-tested |
| Clamped frame timing | implemented, unit-tested |
| `wgpu` device/surface management, resize, surface recovery | implemented, runtime-verified |
| Clear color + sprites (flat or textured, one instanced draw call per texture/sampler run) | implemented, runtime-verified |
| PNG loading to RGBA8 and named sprite-sheet frames (Aseprite JSON subset) | implemented, runtime-verified |
| Texture uploads with declared encoding (linear or sRGB) | implemented, runtime-verified |
| Rotation, tinting, and alpha blending | implemented, runtime-verified |
| Per-sprite sampling (linear or nearest) | implemented, runtime-verified |
| Minimal 2D camera (position + zoom, follow or pixel space) | implemented, runtime-verified |
| Pixel-perfect mode (integer zoom, physical-pixel snapping) | implemented, runtime-verified |
| Engine facade (`mikan`): one lifecycle for renderer creation, resize, errors | implemented, runtime-verified |
| ECS-lite (`mikan-ecs`): generational entities, sparse-set components, resources, ordered systems | implemented, runtime-verified |
| 2D collision (`mikan-collision`): uniform-grid broad phase, AABB resolution, C99 narrow phase | implemented, runtime-verified |
| Gameplay scripting (`mikan-script`): sandboxed Luau step contract | implemented, runtime-verified |
| Frame statistics (sprites/batches/draw calls) and a sprite benchmark | implemented, measured |
| Offscreen frame capture (PPM) for tests and tooling | implemented, runtime-verified |
| 3D, audio, physics (impulses/friction beyond AABB resolution), hot reload | **not implemented** — see the roadmap |

## Quick start

```bash
git clone https://github.com/neverseeyouagainnnnn/mikan
cd mikan
cargo run -p demo2d --release
```

The demo opens a 960x600 window and loads its sprites from
`games/demo2d/assets` — a committed PNG atlas with named frames, loaded through
`mikan-assets` and uploaded as sRGB textures. Three scenes are available:

```bash
cargo run -p demo2d --release                          # classic: drive the square
cargo run -p demo2d --release -- --scene showcase      # rotation, tint, alpha, samplers, PNG alpha
cargo run -p demo2d --release -- --scene stress --sprites 10000   # 10,000-sprite benchmark scene
cargo run -p demo2d --release -- --pixel-perfect 240x150          # integer-zoomed pixel-perfect view
```

| Input | Action |
| --- | --- |
| `W` `A` `S` `D` or arrow keys | Move the square |
| `Escape` | Quit |
| Window close button | Quit |

The square is a `mikan-ecs` entity simulated at a fixed 120 Hz and drawn
interpolated, so motion stays smooth even when the display refresh rate does
not divide the simulation rate evenly. Its movement decisions come from a Luau
gameplay script (`--script`, default `games/demo2d/scripts/player.luau`) and its
collisions against the scene's three static blocks are resolved by
`mikan-collision` once per fixed step. `cargo run -p demo2d -- --help` lists
every option; see [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) for the full
command reference, including headless smoke-test and benchmark modes.

## Workspace layout

```
crates/mikan                engine facade: the application lifecycle that composes the engine crates
crates/mikan-core           configuration, time, fixed timestep, logging, 2D camera, shared types
crates/mikan-assets         PNG decoding and sprite-sheet metadata (CPU only, no graphics)
crates/mikan-ecs            entity/component storage, resources, and system scheduling (CPU only)
crates/mikan-collision      2D collision: uniform-grid broad phase, AABB contacts, Rust reference math
crates/mikan-collision-sys  the C99 narrow-phase kernel and its ABI mirror (the one unsafe crate)
crates/mikan-script         sandboxed Luau VM and the gameplay step contract
crates/mikan-platform       window lifecycle, input state, application runner (winit)
crates/mikan-render         device and swap chain management, sprites and textures, capture (wgpu)
games/demo2d                playable 2D demo built on the engine runtime
docs/                       architecture, development guide, FFI policy, sprite-sheet format, ECS guide, collision and scripting guides, ADRs
```

Dependency direction is one-way and enforced by the manifests:

```
                    mikan-core                mikan-ecs
                        |                         |
        +---------------+                         |
        |               |                         |
 mikan-assets    mikan-platform            mikan-render
        |               |                         |
        +---------------+-------------------------+
                        |
                      mikan                   (facade: Game, Engine, run)
                        |
                     demo2d                   (depends only on mikan)

also under mikan:  mikan-collision (safe) --> mikan-collision-sys (C99 kernel)
                   mikan-script (mlua + vendored Luau VM, C++)
```

`mikan-platform` and `mikan-render` are siblings: the renderer knows nothing
about event handling, and the platform layer knows nothing about graphics APIs.
The facade composes them without either layer knowing about the other; they
still meet directly in applications that wire the layers by hand. See
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the reasoning.

## Engine crates

- **`mikan`** — the facade an application depends on: the `Game` lifecycle
  trait, the `Engine` handle (platform context plus renderer), and `mikan::run`.
  It creates the renderer, forwards resizes, translates errors, and re-exports
  the layers' common types.
- **`mikan-core`** — platform-independent fundamentals: `EngineConfig`, `Color`,
  `Time`, `FixedTimestep`, `FrameOutcome`, `Camera2d` (including the
  pixel-perfect constructor), `UvRect`, logging initialization.
- **`mikan-assets`** — `Image` (PNG decoded to RGBA8 with limits and descriptive
  errors) and `SpriteSheet` (named frames from a documented Aseprite JSON
  subset, validated and converted to inset UV rectangles). No window, no
  device.
- **`mikan-ecs`** — entities (generational handles that cannot go stale),
  components in insertion-ordered sparse sets, resources, and a schedule that
  runs named systems once each in registration order. No window, no device, no
  workspace dependencies; the guide is [`docs/ecs.md`](docs/ecs.md).
- **`mikan-collision`** — 2D collision: `Aabb`/`Pair`/`Contact`, a uniform-grid
  broad phase with deterministic output, one safe batch call into the C99
  narrow-phase kernel (`mikan-collision-sys`), and a Rust `reference`
  implementation that doubles as the test oracle and benchmark baseline. The
  guide is [`docs/collision.md`](docs/collision.md); the honest parity result
  and the decision to use C for this layer are in ADR 0010.
- **`mikan-script`** — sandboxed gameplay scripting: a `ScriptEngine` that
  loads a Luau script and calls its global `mikan_step` once per fixed step,
  with engine-side validation (finite `f32`s, non-negative speed, over-long
  directions normalized), an explicit safe library set, and a 64 MiB memory
  limit. The guide is [`docs/scripting.md`](docs/scripting.md); the decision is
  ADR 0011. A C++ compiler is required to build it (the vendored Luau VM).
- **`mikan-platform`** — the app lifecycle (`App`/`AppContext`), input snapshot
  (`InputState`), and the window runner. It owns the only windowing dependency.
- **`mikan-render`** — `Renderer`: instance, adapter, device, queue, swap chain,
  sprites (rotation, tint, alpha, per-sprite sampling) and textures
  (atlas-ready UV rectangles, declared encoding), the camera matrix, frame
  statistics, and optional frame readback. It owns the only graphics
  dependency.

## Documentation

| Document | Contents |
| --- | --- |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Goals, crate responsibilities, lifecycle, error strategy |
| [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) | Prerequisites, build, test, run, headless verification, benchmark protocol |
| [`docs/sprite-sheets.md`](docs/sprite-sheets.md) | The accepted sprite-sheet JSON, validation, and the half-texel UV inset |
| [`docs/ecs.md`](docs/ecs.md) | The ECS-lite guide: entities, components, resources, systems, determinism |
| [`docs/collision.md`](docs/collision.md) | The collision guide: broad phase, AABB semantics, the C kernel and its boundary, demo integration |
| [`docs/scripting.md`](docs/scripting.md) | The Luau guide: the step contract, validation, sandbox, failure semantics, demo integration |
| [`docs/ffi.md`](docs/ffi.md) | Native code policy and boundary rules |
| [`docs/STATUS.md`](docs/STATUS.md) | What is implemented, tested, and pending |
| [`docs/adr/`](docs/adr/) | Architecture decision records |

## Roadmap

The ECS-lite layer landed first ([`docs/ecs.md`](docs/ecs.md)); Phase 5 then
landed 2D collision detection and resolution ([`docs/collision.md`](docs/collision.md),
ADR 0010) together with an owner-directed addition, Luau gameplay scripting
([`docs/scripting.md`](docs/scripting.md), ADR 0011 — a deliberate roadmap
amendment recorded as its own decision, not a silent substitution).

Next milestones, in order of intended value:

1. Asset pipeline depth: atlas packing and hot reload on top of `mikan-assets`.
2. Runtime verification on Windows, macOS, and Wayland (ADR 0005), and a
   hardware-GPU measurement of the sprite benchmark.

3D work is deliberately out of scope until the 2D foundation is stable. Native
modules are no longer all deferred: ADR 0010 (the C99 collision kernel) and ADR
0011 (the vendored C++ Luau VM) introduced the first two, each with an explicit
decision and honest measurements per ADR 0004 and [`docs/ffi.md`](docs/ffi.md);
ADRs 0006–0009 record why the earlier milestones were Rust-only.

## Platform support

| Platform | Status |
| --- | --- |
| Linux (X11, Wayland) | built and runtime-verified on X11 with a software Vulkan device |
| Windows, macOS | expected to build; **not yet verified** in this repository |

## License

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE))
- MIT license ([`LICENSE-MIT`](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
