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
by explicitly ordered systems and a Luau gameplay script, links entities into a
scene transform hierarchy (a sun, planets, and a moon that orbit rigidly, with
deferred despawn), resolves AABB collisions against the scene's static blocks,
plays WAV sounds through a 64-voice mixer (a real output device when one exists,
a deterministic offline backend otherwise), handles resize and every
recoverable swap-chain condition, reports draw statistics, and exits cleanly.

| Capability | State |
| --- | --- |
| Window + event loop (winit) | implemented, runtime-verified |
| Fixed-timestep simulation with interpolation | implemented, unit-tested |
| Keyboard/mouse state with press/release edges | implemented, unit-tested |
| Action mapping with prioritized contexts over the native input backend | implemented, integration-tested |
| WAV decoding (strict subset: PCM/float, mono/stereo) | implemented, unit-tested |
| Audio mixing (`mikan-audio`): 64 voices, pitch, volume, loops, buses/master, C device backend + C++ mixer | implemented, runtime-verified (offline mixer; no sound card on this machine) |
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
| Scenes (`mikan-scene`): transform hierarchy, world transforms, one active scene, deferred despawn | implemented, runtime-verified |
| Gameplay scripting (`mikan-script`): sandboxed Luau step contract | implemented, runtime-verified |
| Action mapping (`mikan-input`): contexts, priorities, per-action state, C device backend + C++ runtime | implemented, runtime-verified |
| Frame statistics (sprites/batches/draw calls) and a sprite benchmark | implemented, measured |
| Offscreen frame capture (PPM) for tests and tooling | implemented, runtime-verified |
| 3D, physics (impulses/friction beyond AABB resolution), audio streaming/DSP, hot reload | **not implemented** — see the roadmap |

## Quick start

```bash
git clone https://github.com/mikanengine/mikan
cd mikan
cargo run -p demo2d --release
```

The demo opens a 960x600 window and loads its sprites from
`games/demo2d/assets` — a committed PNG atlas with named frames, loaded through
`mikan-assets` and uploaded as sRGB textures. Four scenes are available:

```bash
cargo run -p demo2d --release                          # classic: drive the square
cargo run -p demo2d --release -- --actions             # same controls through the action-mapping system
cargo run -p demo2d --release -- --audio               # a blip on every movement edge, through the mixer
cargo run -p demo2d --release -- --scene hierarchy     # sun, orbiting planets, and a moon
cargo run -p demo2d --release -- --scene showcase      # rotation, tint, alpha, samplers, PNG alpha
cargo run -p demo2d --release -- --scene stress --sprites 10000   # 10,000-sprite benchmark scene
cargo run -p demo2d --release -- --pixel-perfect 240x150          # integer-zoomed pixel-perfect view
```

| Input | Action |
| --- | --- |
| `W` `A` `S` `D` or arrow keys | Move the square (classic) |
| `A` / `D` | Spin the sun, orbiting the hierarchy scene |
| `F` | Despawn the moon (hierarchy) |
| `Escape` | Quit |
| Window close button | Quit |

The square is a `mikan-ecs` entity simulated at a fixed 120 Hz and drawn
interpolated, so motion stays smooth even when the display refresh rate does
not divide the simulation rate evenly. Its movement decisions come from a Luau
gameplay script (`--script`, default `games/demo2d/scripts/player.luau`) and its
collisions against the scene's three static blocks are resolved by
`mikan-collision` once per fixed step. The hierarchy scene's sun, planets, and
moon are entities linked by `mikan-scene`'s transform hierarchy, so spinning the
sun orbits them rigidly and `F` removes the moon through the deferred-despawn
queue. `cargo run -p demo2d -- --help` lists every option; see
[`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) for the full command reference,
including headless smoke-test and benchmark modes. The action-input
architecture, fixed-step semantics, supported devices, and gamepad limitation
are documented in [`docs/input.md`](docs/input.md); the audio architecture, the
WAV subset, the offline backend, and the measured mixer cost are documented in
[`docs/audio.md`](docs/audio.md).

## Workspace layout

```
crates/mikan                engine facade: the application lifecycle that composes the engine crates
crates/core           configuration, time, fixed timestep, logging, 2D camera, shared types
crates/assets         PNG decoding and sprite-sheet metadata (CPU only, no graphics)
crates/ecs            entity/component storage, resources, and system scheduling (CPU only)
crates/collision      2D collision: uniform-grid broad phase, AABB contacts, Rust reference math
crates/collision-sys  the C99 narrow-phase kernel and its ABI mirror (one of the unsafe crates)
crates/scene          scenes: transforms, the entity hierarchy, deferred despawn, one active scene
crates/scene-sys      the C transform kernel and the C++ scene graph (one of the unsafe crates)
crates/input          action mapping: actions, prioritized contexts, per-action state (safe face)
crates/input-sys      the C device backend and the C++ action runtime (one of the unsafe crates)
crates/audio          audio: WAV sound data and the safe mixer API (safe face)
crates/audio-sys      the C device backend over vendored miniaudio and the C++ mixer (one of the unsafe crates)
crates/script         sandboxed Luau VM and the gameplay step contract
crates/platform       window lifecycle, input state, application runner (winit)
crates/render         device and swap chain management, sprites and textures, capture (wgpu)
games/demo2d                playable 2D demo built on the engine runtime
docs/                       architecture, development guide, FFI policy, sprite-sheet format, ECS, collision, scene, input, audio, and scripting guides, ADRs
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
                   mikan-scene (safe)     --> mikan-scene-sys (C kernel + C++ graph)
                   mikan-input (safe)     --> mikan-input-sys (C backend + C++ runtime)
                   mikan-audio (safe)     --> mikan-audio-sys (C backend over miniaudio + C++ mixer)
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
  errors), `SpriteSheet` (named frames from a documented Aseprite JSON subset,
  validated and converted to inset UV rectangles), and `SoundData` (WAV decoded
  from a documented strict subset: PCM 8/16/24/32 and float 32/64, mono/stereo,
  8–192 kHz, with a frame cap and no partial decodes). No window, no device.
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
- **`mikan-scene`** — scenes and the entity transform hierarchy: `Transform2d`
  and `GlobalTransform2d`, a parent/child `Hierarchy` (a forest with cycle
  rejection and re-root-on-removal), a `DespawnQueue`, a `Scene` (one world,
  one schedule, ordered update), and a `SceneManager` (generational ids, one
  active scene). The hierarchy's topology is a hand-written C++ graph and its
  composition a C kernel (`mikan-scene-sys`, the other crate allowed `unsafe`);
  a Rust `reference` compose is the bit-exact test oracle and benchmark
  baseline. The guide is [`docs/scene.md`](docs/scene.md); the decision and the
  honest (slower-than-Rust) measurement are in ADR 0012.
- **`mikan-input`** — action mapping: `InputSystem` turns drained window
  events into action state (`held`, `just_pressed`, `just_released`,
  `repeating`, `value`), bindings are grouped into prioritized contexts with
  per-event arbitration, and `Binding` covers keys, mouse buttons, and wheel
  axes. The device backend is a C kernel and the action runtime a hand-written
  C++ state machine (`mikan-input-sys`, the other crate allowed `unsafe`); a
  Rust `reference` normalizer is the bit-exact test oracle. The guide is
  [`docs/input.md`](docs/input.md); the decision and the honest
  (slower-than-Rust) measurement are in ADR 0013.
- **`mikan-audio`** — audio playback: `AudioSystem` (a real output device or a
  deterministic offline backend), `Sound`/`Voice` generation-checked handles,
  `PlaySettings`/`Bus`, and offline `drain_samples`/`overruns` for headless
  loops. The device backend is C (over vendored miniaudio, its silent null
  backend deliberately disabled) and the mixer a hand-written C++ runtime with
  fixed pools and a lock-free command queue (`mikan-audio-sys`, the other crate
  allowed `unsafe`); a Rust `reference` mixer is the bit-exact test oracle. The
  guide is [`docs/audio.md`](docs/audio.md); the decision and the measured
  realtime-budget result are in ADR 0014 and `docs/STATUS.md`.
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
| [`docs/scene.md`](docs/scene.md) | The scene guide: transforms, hierarchy invariants, scene update order, deferred despawn, the native boundary and its honest measurement |
| [`docs/input.md`](docs/input.md) | The input guide: the event model, contexts and priorities, action states, the C/C++ boundary and its honest measurement |
| [`docs/audio.md`](docs/audio.md) | The audio guide: the mixer and its limits, handles, the offline backend, the WAV subset, the measured realtime headroom |
| [`docs/scripting.md`](docs/scripting.md) | The Luau guide: the step contract, validation, sandbox, failure semantics, demo integration |
| [`docs/ffi.md`](docs/ffi.md) | Native code policy and boundary rules |
| [`docs/STATUS.md`](docs/STATUS.md) | What is implemented, tested, and pending |
| [`docs/adr/`](docs/adr/) | Architecture decision records |

## Roadmap

The ECS-lite layer landed first ([`docs/ecs.md`](docs/ecs.md)); Phase 5 then
landed 2D collision detection and resolution ([`docs/collision.md`](docs/collision.md),
ADR 0010) together with an owner-directed addition, Luau gameplay scripting
([`docs/scripting.md`](docs/scripting.md), ADR 0011 — a deliberate roadmap
amendment recorded as its own decision, not a silent substitution). Phase 7
added the scene and entity transform hierarchy
([`docs/scene.md`](docs/scene.md), ADR 0012), whose topology is a hand-written
C++ graph and whose composition is a C kernel. The input milestone then landed
action mapping ([`docs/input.md`](docs/input.md), ADR 0013): a C device backend
and a hand-written C++ action runtime behind one platform event log. Phase 9
landed audio ([`docs/audio.md`](docs/audio.md), ADR 0014 — another
owner-directed amendment): a C device backend over vendored miniaudio and a
hand-written C++ mixer, with a deterministic offline backend for machines (and
CI runners) without a sound card.

Next milestones, in order of intended value:

1. Asset pipeline depth: atlas packing and hot reload on top of `mikan-assets`,
   plus a streaming audio path.
2. Runtime verification on Windows, macOS, and Wayland (ADR 0005), a
   hardware-GPU measurement of the sprite benchmark, and the first runs on a
   machine with a real sound card (plus the pending Valgrind record).

3D work is deliberately out of scope until the 2D foundation is stable. Native
modules are no longer all deferred: ADR 0010 (the C99 collision kernel), ADR
0011 (the vendored C++ Luau VM), ADR 0012 (the C transform kernel and the
C++ scene graph), ADR 0013 (the C input backend and C++ action runtime), and
ADR 0014 (the C audio backend over vendored miniaudio and the C++ mixer)
introduced the first five, each with an explicit decision and
honest measurements per ADR 0004 and [`docs/ffi.md`](docs/ffi.md); ADRs 0006–0009
record why the earlier milestones were Rust-only.

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
