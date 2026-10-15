# Development

How to build, test, run, and verify Mikan on your machine.

## Prerequisites

### All platforms

- **Rust**: install with [rustup](https://rustup.rs). The toolchain version and
  components (rustfmt, clippy) are pinned in `rust-toolchain.toml`; rustup
  installs them automatically on the first `cargo` invocation in this repository.
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
the window quits. The demo exits with a non-zero status and a readable message if
initialization or rendering fails.

The demo renders a small procedural texture atlas (a player tile and a marker
tile) and can render through a camera:

```bash
cargo run -p demo2d --release -- --camera follow        # camera tracks the player
cargo run -p demo2d --release -- --camera follow --zoom 2   # zoomed in 2x
```

`--zoom` requires a camera; it is rejected on its own rather than silently
ignored. The default is `--camera none`, which renders in pixel space exactly as
the pre-camera milestone did.

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

The script also runs the headless smoke test, the capture, input, and camera
verification below when `xvfb-run` (and, for the latter two, `xdotool`) is
available, and skips them with a notice when it is not.

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
2. `wgpu` finds an adapter and creates a device (with `lavapipe`, a CPU device).
3. The swap chain is configured, 120 frames are rendered and presented with no
   unrecovered surface condition.
4. `--capture` copies the last frame back to CPU memory and writes it.
5. The process exits with status 0 — and with a non-zero status, after the
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

### Frame-time statistics

`--stats` prints mean, p50, p95, and max frame times and the implied average
frame rate. Read them with these caveats:

- Build with `--release`. Debug builds of `wgpu` are not representative of
  anything.
- On CI or `lavapipe`, numbers describe a software rasterizer, not your GPU.
- Vsync is on by default, so frame times cluster at the display refresh interval
  (about 16.7 ms at 60 Hz). That measures the compositor, not the engine.

For engine cost measurements, use a real GPU, disable vsync
(`EngineConfig::vsync`), hold the scene identical across runs, and report the
hardware, driver, resolution, and frame count next to the numbers. Never quote a
number produced on a different machine as if it applied to this one.

## Repository conventions

- Formatting and lint rules live in the workspace manifest (`[workspace.lints]`)
  and apply to every crate. New crates add `[lints] workspace = true`.
- Every public item has a doc comment; `missing_docs` is a warning and CI treats
  warnings as errors.
- `unsafe_code` is forbidden workspace-wide. This is a deliberate constraint:
  if a change needs `unsafe`, it needs an accepted ADR first.
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
