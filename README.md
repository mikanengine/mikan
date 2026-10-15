# mikan

Mikan 蜜柑 — a small, modular game engine. 2D first, then 3D. Rust at the core, C at the foundation, C++ where it counts.

> 小さな房が、世界になる。
> Small segments, whole worlds.

## What works today

This is an early but real vertical slice: a windowed application that renders at
vsync, draws flat or textured sprites through a minimal 2D camera, reads the
keyboard, runs a fixed-timestep simulation, handles resize and every recoverable
swap-chain condition, and exits cleanly.

| Capability | State |
| --- | --- |
| Window + event loop (winit) | implemented, runtime-verified |
| Fixed-timestep simulation with interpolation | implemented, unit-tested |
| Keyboard/mouse state with press/release edges | implemented, unit-tested |
| Clamped frame timing | implemented, unit-tested |
| `wgpu` device/surface management, resize, surface recovery | implemented, runtime-verified |
| Clear color + sprites (flat or textured, one instanced draw call per texture run) | implemented, runtime-verified |
| Textures uploaded from linear RGBA8 data, atlas UV rectangles | implemented, runtime-verified |
| Minimal 2D camera (position + zoom, follow or pixel space) | implemented, runtime-verified |
| Engine facade (`mikan`): one lifecycle for renderer creation, resize, errors | implemented, runtime-verified |
| Offscreen frame capture (PPM) for tests and tooling | implemented, runtime-verified |
| 3D, audio, physics, ECS, asset loading | **not implemented** — see the roadmap |

## Quick start

```bash
git clone https://github.com/neverseeyouagainnnnn/mikan
cd mikan
cargo run -p demo2d --release
```

The demo opens a 960x600 window with a dark background, an orange textured
square, and a blue marker in the corner. Both are tiles of a small atlas the
demo generates in code; nothing is loaded from disk.

| Input | Action |
| --- | --- |
| `W` `A` `S` `D` or arrow keys | Move the square |
| `Escape` | Quit |
| Window close button | Quit |

The square is simulated at a fixed 120 Hz and drawn interpolated, so motion
stays smooth even when the display refresh rate does not divide the simulation
rate evenly. See [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) for the full
command reference, including headless smoke-test mode.

## Workspace layout

```
crates/mikan            engine facade: the application lifecycle that composes platform and rendering
crates/mikan-core       configuration, time, fixed timestep, logging, 2D camera, shared types
crates/mikan-platform   window lifecycle, input state, application runner (winit)
crates/mikan-render     device and swap chain management, sprites and textures, capture (wgpu)
games/demo2d            playable 2D demo built on the engine runtime
docs/                   architecture, development guide, FFI policy, ADRs
```

Dependency direction is one-way and enforced by the manifests:

```
                         mikan-core
                         ^        ^
                         |        |
               mikan-platform   mikan-render
                         ^        ^
                          \      /
                            mikan          (facade: Game, Engine, run)
                              ^
                              |
                           demo2d          (depends only on mikan)
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
  `Time`, `FixedTimestep`, `FrameOutcome`, `Camera2d`, logging initialization.
- **`mikan-platform`** — the app lifecycle (`App`/`AppContext`), input snapshot
  (`InputState`), and the window runner. It owns the only windowing dependency.
- **`mikan-render`** — `Renderer`: instance, adapter, device, queue, swap chain,
  sprites and textures (atlas-ready UV rectangles), the camera matrix, and
  optional frame readback. It owns the only graphics dependency.

## Documentation

| Document | Contents |
| --- | --- |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Goals, crate responsibilities, lifecycle, error strategy |
| [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) | Prerequisites, build, test, run, headless verification |
| [`docs/ffi.md`](docs/ffi.md) | Native code policy and boundary rules |
| [`docs/STATUS.md`](docs/STATUS.md) | What is implemented, tested, and pending |
| [`docs/adr/`](docs/adr/) | Architecture decision records |

## Roadmap

Next milestones, in order of intended value:

1. ECS-lite: entity storage and systems with explicit scheduling.
2. 2D collision detection and resolution.
3. Asset loading and hot reload.
4. Runtime verification on Windows, macOS, and Wayland (ADR 0005).

3D work, native C modules, and C++ integrations are deliberately out of scope
until the 2D foundation is stable. Each of those needs a concrete justification
before it is started; see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), ADR
0004, and ADRs 0006 and 0007 (the sprites/camera and facade milestones record
why they are Rust-only).

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
