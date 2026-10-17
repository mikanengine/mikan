# Development

How to build, test, run, and verify Mikan on your machine.

## Prerequisites

### All platforms

- **Rust**: install with [rustup](https://rustup.rs). The toolchain version and
  components (rustfmt, clippy) are pinned in `rust-toolchain.toml`; rustup
  installs them automatically on the first `cargo` invocation in this repository.
- **A C compiler and a C++ compiler.** The collision kernel is C99
  (`crates/collision-sys`), the scene module has a C compose kernel and a C++
  scene graph (`crates/scene-sys`), and the vendored Luau VM is C++
  (`crates/script`) — all compiled as part of `cargo build`. On Debian
  and Ubuntu, `sudo apt-get install -y build-essential` (or `gcc g++`)
  covers it; on macOS, the Xcode Command Line Tools; on Windows, the MSVC
  Build Tools that a normal Rust install already uses.
- **A Vulkan, Metal, DirectX 12, or OpenGL ES capable GPU.**
  `wgpu` falls back through available backends automatically. If your machine has
  no hardware support, a software rasterizer also works (see below).

### Linux

Windowing goes through `winit`, which loads the X11 and Wayland client
libraries **at runtime** rather than linking them. The runtime packages are what
matter; development headers are not required to build Mikan. On Debian and
Ubuntu:

```bash
sudo apt-get install -y \
  libx11-6 libx11-xcb1 libxcursor1 libxi6 libxrandr2 \
  libxkbcommon0 libxkbcommon-x11-0
```

If any of these are missing, the window cannot be created and the process exits
with:

```
event loop failure: ... Failed to load one of xlib's shared libraries
```

For headless machines (CI, containers, remote shells without a display):

```bash
sudo apt-get install -y xvfb mesa-vulkan-drivers
```

`mesa-vulkan-drivers` provides `lavapipe`, a software Vulkan implementation, so
rendering can be exercised without a GPU. Together with the list above, this is
exactly what the CI smoke test installs.

### Windows and macOS

Nothing beyond Rust and a graphics driver. `winit` uses the native windowing
APIs and `wgpu` uses Metal on macOS and DX12/Vulkan on Windows.

## Build and run

```bash
cargo build                            # debug build of the whole workspace
cargo run -p demo2d --release          # run the demo (optimized)
cargo run -p demo2d -- --help          # command-line reference
```

Demo controls: `WASD` or arrow keys move the square, `Escape` quits, and closing
the window quits. In the `hierarchy` scene, `A`/`D` spin the sun (the planets
and moon follow rigidly) and `F` despawns the moon. The demo exits with a
non-zero status and a readable message if initialization or rendering fails.

The demo is an application of the `mikan` facade crate (`crates/mikan`): it
implements `mikan::Game` and depends on no other engine crate. The facade
creates the renderer, forwards resizes, and translates errors; see
[`ARCHITECTURE.md`](ARCHITECTURE.md) §2 for the layer split.

The demo loads its sprites from `games/demo2d/assets` — an `atlas.png` with a
committed `atlas.json` sprite sheet and a `star.png` with transparent corners —
decoded by `mikan-assets` and uploaded as sRGB textures at startup. A missing,
corrupt, or incomplete asset set stops the demo with a readable error before
the first frame; `--assets DIR` loads a different directory. The files are
generated (and checked in CI) by:

```bash
python3 scripts/generate_assets.py          # rewrite games/demo2d/assets
python3 scripts/generate_assets.py --check  # fail if the committed files differ
```

Four scenes share the loaded assets:

```bash
cargo run -p demo2d --release -- --scene classic    # default: drive the square
cargo run -p demo2d --release -- --scene hierarchy  # spin a sun; its planets and moon orbit it
cargo run -p demo2d --release -- --scene showcase   # one feature per fixed position
cargo run -p demo2d --release -- --scene stress --sprites 10000 --no-vsync --stats
```

In the `hierarchy` scene the sun, three planets, and a moon are entities linked
by the scene's transform hierarchy (the guide is [`scene.md`](scene.md)): `A`/`D`
spin the sun and the planets orbit rigidly, and `F` despawns the moon through the
deferred queue.

and the camera options are:

```bash
cargo run -p demo2d --release -- --camera follow            # camera tracks the player
cargo run -p demo2d --release -- --camera follow --zoom 2   # zoomed in 2x
cargo run -p demo2d --release -- --pixel-perfect 240x150    # integer-zoomed pixel-perfect view
cargo run -p demo2d --release -- --scene showcase --sampler nearest  # force one sampler scene-wide
```

`--zoom` requires a camera; it is rejected on its own rather than silently
ignored, and it cannot be combined with `--pixel-perfect` (the zoom is derived
from the virtual resolution there). The default is `--camera none`, which
renders in pixel space exactly as the pre-camera milestone did. `--pixel-perfect
WxH` derives the largest integer zoom at which a WxH virtual screen fits the
window (4x for 240x150 in the default 960x600 window) and snaps the camera to
the physical pixel grid; its exact contract is documented in ADR 0008.

The classic player's movement decisions come from a Luau gameplay script, loaded
through `mikan-script`:

```bash
cargo run -p demo2d --release                                        # the default script
cargo run -p demo2d --release -- --script games/demo2d/scripts/player.luau
cargo run -p demo2d --release -- --script /path/to/my_player.luau    # your own
```

The script's step contract (arguments, return values, validation, failure
behavior) and the sandbox are documented in [`scripting.md`](scripting.md); the
decision record is ADR 0011.

## Quality gates

Run these before pushing; CI runs the same set and fails on any deviation.

```bash
cargo fmt --all -- --check                      # formatting
cargo check --workspace --all-targets           # types, including tests
cargo test --workspace                          # unit and doc tests
cargo clippy --workspace --all-targets -- -D warnings   # lints as errors
cargo build --workspace --release               # release build
```

Or run all of them, plus the headless runtime checks, with one command:

```bash
./scripts/verify.sh
```

The script also runs the generator check and the headless smoke test, capture,
input, camera, ecs, collision, scripting, scene, showcase, stress,
pixel-perfect, and asset verification below when `xvfb-run` is available, and
skips the display-dependent ones with a notice when it is not.

Logging is controlled with the standard `RUST_LOG` environment variable:

```bash
RUST_LOG=debug cargo run -p demo2d --release     # engine diagnostics
RUST_LOG=mikan_render=trace cargo run -p demo2d  # renderer internals only
```

## Headless smoke test

The demo can present a fixed number of frames and exit on its own, which makes it
usable as an automated check. On a machine without a display:

```bash
mkdir -p artifacts
xvfb-run -a cargo run -p demo2d --release -- \
    --frames 120 \
    --capture artifacts/frame.ppm \
    --stats
```

What this proves, in order:

1. The window is created and the event loop runs under a virtual X server.
2. The demo loads its sprite assets from `games/demo2d/assets` (run commands
   from the repository root, or pass `--assets DIR`).
3. `wgpu` finds an adapter and creates a device (with `lavapipe`, a CPU device).
4. The swap chain is configured, 120 frames are rendered and presented with no
   unrecovered surface condition.
5. `--capture` copies the last frame back to CPU memory and writes it.
6. The process exits with status 0 — and with a non-zero status, after the
   timeout, if any of the above fails.

`--frames` implies `--timeout 30` so a smoke test cannot hang forever. A failure
to present looks like this on stderr:

```
demo2d: presented 7 of 120 requested frames within 30s; the window or graphics
device is not producing frames
```

### Checking the captured image

`--capture` writes a binary PPM (P6) file: a short ASCII header followed by raw
RGB bytes. To verify the pixels programmatically:

```bash
python3 - <<'PY'
import sys
with open("artifacts/frame.ppm", "rb") as handle:
    data = handle.read()
header, pixels = data.split(b"\n255\n", 1)
_, width, height = header.split()
width, height = int(width), int(height)
def pixel(x, y):
    index = (y * width + x) * 3
    return tuple(pixels[index:index + 3])
print("size:", width, height)
print("background:", pixel(2, 2))
print("center:", pixel(width // 2, height // 2))
PY
```

The center pixel should be the square's orange color and the corner pixel the
dark clear color. The exact values depend on the swap chain format (sRGB
encodes linear colors to the display space), so treat this as a qualitative
check: the three channels of the center pixel must differ from the background.

### Verifying keyboard input

The smoke test never presses a key, so input is verified separately by injecting
real X11 key events with `xdotool`:

```bash
xvfb-run -a bash scripts/verify_input.sh
```

The script holds `D` for one second and checks that the square ended up where
the configured speed says it should, then taps `Escape` and checks that the
process exits cleanly with status 0 instead of running into the frame-budget
timeout. It requires `xdotool`, `python3`, and a display.

This check exists because it found a real bug: with rendering at roughly
600 frames per second and a 120 Hz simulation, most frames run zero fixed
updates, and input press edges were being discarded at the end of those frames.
A tapped Escape key was therefore lost. The runner now consumes input edges only
on frames that ran an update.

### Verifying the 2D camera

The follow camera and `--zoom` are verified the same way — by moving the player
and looking at what was actually rendered:

```bash
xvfb-run -a bash scripts/verify_camera.sh
```

The script holds `D` for one second and then checks two things at once: the
demo log must report the square at its moved *world* position, and the captured
frame must show the square at the *centre of the screen*. If the camera did not
follow, the square would sit near the right edge of the capture instead. It then
renders with `--zoom 2` and checks that the square covers roughly twice the
pixels.

Both checks find the square by colour (`scripts/verify_camera.py`), so they also
exercise the texture path: the player tile has a darker border and a diagonal
highlight, and a diagonal edge cannot come from an axis-aligned flat rectangle.

### Verifying the ECS

The classic scene's player is a `mikan-ecs` entity whose systems run once per
fixed step; `scripts/verify_ecs.sh` checks that the schedule is what moved it:

```bash
xvfb-run -a bash scripts/verify_ecs.sh
```

It requires the startup log to list the world (four entities — the player and
the three collision blocks — eleven components, and the system order
`snapshot, input, movement, collide, clamp`); then holds `D` and requires
the shutdown log's moved-step counter and final position to agree exactly:
every moved step advanced 3.5 px (420 px/s divided by the 120 Hz fixed step)
from the start centre, until the right wall clamps the player (the step that
reaches the wall counts as moved; steps pinned against it change nothing and
do not). Finally it runs with no input at all and requires the counter to stay
at zero and the square to sit at the start centre. That is what "systems are
scheduled on the fixed timestep, exactly once per step" means as an observable
fact. The guide for the layer itself is [`ecs.md`](ecs.md).

### Verifying collision

```bash
xvfb-run -a bash scripts/verify_collision.sh
```

The classic scene has three static collision blocks; the player is a collider
too, and the `collide` system resolves overlaps once per fixed step. The script
requires the startup log to report the blocks and the broad-phase grid, then
holds down+right and requires the diagonal run to stop *against the first
pillar's left face* (x = 582) and slide down it to the floor clamp (y = 552):
passing through the block, stopping at first contact, or being ejected along
the wrong axis all fail. A straight run along y = 300 afterwards must be
unobstructed, which guards the corridor the input and camera verifications
walk along. The layer guide is [`collision.md`](collision.md); the decision
record (including the C kernel's boundary and the honest parity result) is
ADR 0010.

### Verifying gameplay scripting

```bash
xvfb-run -a bash scripts/verify_script.sh
```

The demo's movement now *is* the script, so the check starts by proving that:
the startup log must name the loaded script, and holding `D` must move exactly
3.5 px per moved fixed step — the same numbers the Rust implementation
produced before the move to Luau. It then loads a fixture script that returns
`(3, 4, 60)` for exactly 60 steps and requires the player to trace the
*normalized* `(0.6, 0.8)` path to `[498, 324]` (an unnormalized engine would
end at `[570, 420]`; a broken step counter would clamp at `[912, 552]` —
all four outcomes are distinguishable). Finally every documented failure path
is exercised: a syntax error, a missing `mikan_step`, and a missing file must
each fail startup with a clear message and a non-zero exit; a runtime error and
a non-finite return must each be logged exactly once, move nothing, and leave
the game running. The guide is [`scripting.md`](scripting.md); the decision
record is ADR 0011.

### Verifying the scene hierarchy

```bash
xvfb-run -a bash scripts/verify_scene.sh
```

`--scene hierarchy` is a sun, three planets, and a moon linked as entities by
`mikan-scene`; the check proves the hierarchy propagates and that deferred
despawn works, entirely from the rendered frame. With no input, the sun must be
warm at `[480, 300]`, the first planet blue at `[640, 300]`, and the moon a
yellow star at `[720, 300]` (so the local-translation chain composed correctly).
Holding `D` spins the sun, and the star's centroid must stay exactly one orbit
radius (240 px) from the sun while its angle changes by more than 5° — a
rigid-rotation signature that a broken scale/rotate/translate order cannot
produce. Pressing `F` must remove the star pixels while the sun and planets
remain, so the moon alone was despawned. The layer guide is
[`scene.md`](scene.md); the decision record is ADR 0012.

### Verifying the showcase scene

`--scene showcase` draws eight sprites at fixed positions, one per sprite
feature, so a single captured frame verifies them all:

```bash
xvfb-run -a bash scripts/verify_showcase.sh
```

The script checks the demo log (the assets loaded; 8 sprites in 4 batches and 4
draw calls, one per texture/sampler pair) and then the pixels
(`scripts/verify_showcase.py`): the plain tile's base colour arrives unchanged,
the 45-degree tile reaches pixels an unrotated square cannot paint while its
unrotated box corner is clear, the tinted tile matches the linear tint multiply,
the 0.4-alpha tile blends with the clear color, the linear checker scanline
contains intermediate grays where the nearest one contains only black and
white, and the star is yellow in the middle and transparent at the corners,
where its backdrop shows through.

### Verifying batching at scale

```bash
xvfb-run -a bash scripts/verify_stress.sh                    # 1000 sprites (CI default)
SPRITES=10000 xvfb-run -a bash scripts/verify_stress.sh      # the full benchmark scene
```

The script runs the stress scene, requires the render statistics line to report
exactly one batch and one draw call for the requested sprite count, and checks
that the captured frame is mostly covered in the player tile's colours
(`scripts/verify_stress.py`) — so the sprites were really rasterized, not
merely counted.

### Verifying pixel-perfect mode

```bash
xvfb-run -a bash scripts/verify_pixel_perfect.sh
```

Runs `--pixel-perfect 240x150` in the default 960x600 window, requires the log
to report integer zoom 4, and checks in the capture
(`scripts/verify_pixel_perfect.py`) that the player square is exactly 384 px
(96 virtual pixels times 4) with every bounding-box edge on a multiple of 4.
With a fractional camera position the quad's edges would land between pixels
and the warm bounding box would break that alignment, so this is what "the
whole virtual grid is aligned to the physical grid" means in an image.

### Verifying asset failure modes

```bash
xvfb-run -a bash scripts/verify_assets.sh
```

First runs the generator check, then runs the demo against a missing asset
directory, a sheet whose texture is not a PNG, and a sheet without the
`player` frame. Each run must exit non-zero with the specific message
("could not read", "could not decode PNG data", `has no frame "player"`)
instead of rendering a broken frame.

### Benchmarking the collision kernel

```bash
bash scripts/bench_collision.sh               # 512 boxes / 250,000 candidate pairs
PAIRS=100000 bash scripts/bench_collision.sh  # a different pair load
```

Runs the same candidate pairs through the C narrow-phase kernel
(`mikan-collision-sys`) and the Rust reference implementation
(`mikan_collision::reference`) in release, records each run's ns/pair under
`artifacts/bench/`, and prints both. The recorded result is **parity within
run-to-run noise** — one run favors C, the next favors Rust — and no speedup is
claimed; see `docs/STATUS.md` for the numbers and ADR 0010 for why the kernel
exists anyway. The Rust reference doubles as the test oracle, so this
benchmark compares two implementations that provably agree.

### Benchmarking the scene module

```bash
bash scripts/bench_scene.sh                 # 100,000-node chain, default repetitions
NODES=20000 bash scripts/bench_scene.sh     # a different hierarchy size
```

Runs the same hierarchy and local transforms through the native scene graph
(`mikan-scene-sys`, the C++ graph driving the C compose kernel) and through a
Rust reference fold (`mikan_scene::reference`) in release, records each run's
ns/node under `artifacts/bench/`, and prints both. The recorded result is that
**the native graph is substantially slower** — it carries real graph bookkeeping
(`unordered_map` index and parent lookups, per-call traversal buffers) that the
reference, with a precomputed parent array, gets for free. The driver was the
architecture directive, not performance, and no speedup is claimed; see
`docs/STATUS.md` for the numbers and ADR 0012 for why the module exists anyway.
The Rust reference doubles as the test oracle, cross-checked bit for bit, so
this benchmark compares two implementations that provably agree.

### Benchmarking the sprite pipeline

```bash
xvfb-run -a bash scripts/bench_sprites.sh               # 100/1000/10000 sprites, 300 frames
xvfb-run -a bash scripts/bench_sprites.sh "1000 10000" 120
```

For each sprite count and window size (960x600 and 320x200 by default) the
script runs the stress scene with vsync off for a fixed frame budget, records
the demo's own per-run statistics under `artifacts/bench/`, and prints a table
(window, sprites, batches, draw calls, frames, mean/p50/p95/max, fps). Two
window sizes are measured because they carry different fill costs. Quote
results only with the device named and labelled as hardware or software
rasterizer; `docs/STATUS.md` holds the current recorded numbers and their
caveats.

### Frame-time statistics

`--stats` prints mean, p50, p95, and max frame times and the implied average
frame rate. Read them with these caveats:

- Build with `--release`. Debug builds of `wgpu` are not representative of
  anything.
- On CI or `lavapipe`, numbers describe a software rasterizer, not your GPU.
- Vsync is on by default, so frame times cluster at the display refresh interval
  (about 16.7 ms at 60 Hz). That measures the compositor, not the engine.

For engine cost measurements, use a real GPU, disable vsync
(`EngineConfig::vsync` or `--no-vsync`), hold the scene identical across runs
(`--scene stress --sprites N` is deterministic), and report the hardware,
driver, resolution, and frame count next to the numbers — the two window sizes
of `scripts/bench_sprites.sh` exist exactly so fill cost and per-sprite cost
can be told apart. Never quote a number produced on a different machine as if
it applied to this one.

## Repository conventions

- Formatting and lint rules live in the workspace manifest (`[workspace.lints]`)
  and apply to every crate. New crates add `[lints] workspace = true`.
- Every public item has a doc comment; `missing_docs` is a warning and CI treats
  warnings as errors.
- `unsafe_code` is forbidden workspace-wide. This is a deliberate constraint:
  if a change needs `unsafe`, it needs an accepted ADR first. The exceptions are
  `crates/collision-sys` (the C ABI boundary recorded in ADR 0010) and
  `crates/scene-sys` (the C compose kernel and C++ scene graph recorded in
  ADR 0012), each with the override and its reason in the crate root; their safe
  wrappers and every consumer contain none.
- Tests live next to the code they test (`#[cfg(test)] mod tests`). Pure logic —
  timing, input edges, image encoding, sprite instance layout, UV and camera
  math — is unit-tested without a GPU or a window.
- Dependencies are pinned in the workspace manifest and the lockfile is
  committed. Adding a dependency requires a reason in the pull request; the
  engine's value is partly in what it does not pull in.

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `error: failed to run custom build command for wgpu-core` | Missing X11/Wayland or Vulkan development packages. Install the packages listed above. |
| `no graphics adapter is available for this window` | No working graphics backend. Install a driver, or `mesa-vulkan-drivers` plus `xvfb-run` for headless use. |
| `Failed to load one of xlib's shared libraries` | Missing X11 runtime libraries. Install the packages listed under Linux prerequisites. |
| `this surface does not support frame capture; omit --capture` | The swap chain cannot be copied from on this backend/format. Drop `--capture`; it does not affect normal rendering. |
| Window opens but nothing moves | The window does not have focus, or the event loop is occluded. Click the window; check `RUST_LOG=debug` output. |
| `thread 'main' panicked at ... winit` on Linux | Running without a display. Use `xvfb-run -a` or run on a desktop session. |
