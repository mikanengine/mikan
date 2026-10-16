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
by explicitly ordered systems, handles resize and every recoverable swap-chain
condition, reports draw statistics, and exits cleanly.

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
| Frame statistics (sprites/batches/draw calls) and a sprite benchmark | implemented, measured |
| Offscreen frame capture (PPM) for tests and tooling | implemented, runtime-verified |
| 3D, audio, physics, hot reload | **not implemented** — see the roadmap |

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
not divide the simulation rate evenly. `cargo run -p demo2d -- --help` lists
every option; see [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) for the full
command reference, including headless smoke-test and benchmark modes.

## Workspace layout

```
crates/mikan            engine facade: the application lifecycle that composes platform and rendering
crates/mikan-core       configuration, time, fixed timestep, logging, 2D camera, shared types
crates/mikan-assets     PNG decoding and sprite-sheet metadata (CPU only, no graphics)
crates/mikan-ecs        entity/component storage, resources, and system scheduling (CPU only)
crates/mikan-platform   window lifecycle, input state, application runner (winit)
crates/mikan-render     device and swap chain management, sprites and textures, capture (wgpu)
games/demo2d            playable 2D demo built on the engine runtime
docs/                   architecture, development guide, FFI policy, sprite-sheet format, ECS guide, ADRs
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
| [`docs/ffi.md`](docs/ffi.md) | Native code policy and boundary rules |
| [`docs/STATUS.md`](docs/STATUS.md) | What is implemented, tested, and pending |
| [`docs/adr/`](docs/adr/) | Architecture decision records |

## Roadmap

[`docs/ecs.md`](docs/ecs.md) documents the ECS-lite layer that landed last:
entities, components, resources, and explicitly ordered systems on the existing
fixed timestep.

Next milestones, in order of intended value:

1. 2D collision detection and resolution: broad phase plus AABB resolution on
   the same fixed step.
2. Asset pipeline depth: atlas packing and hot reload on top of `mikan-assets`.
3. Runtime verification on Windows, macOS, and Wayland (ADR 0005), and a
   hardware-GPU measurement of the sprite benchmark.

3D work, native C modules, and C++ integrations are deliberately out of scope
until the 2D foundation is stable. Each of those needs a concrete justification
before it is started; see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), ADR
0004, and ADRs 0006–0009 (the sprites/camera, facade, sprite-rendering, and
ECS-lite milestones record why they are Rust-only).

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
