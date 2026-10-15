# Mikan status

- **Date:** 2026-10-15
- **Revision:** working tree on top of `main` @ `9eaaed8` ("Add demo2d"), which is
  pushed to `origin/main`. What is on top of that commit is listed under
  [Uncommitted changes](#uncommitted-changes-at-the-time-of-writing).
- **Purpose:** state precisely what is implemented, how each claim was verified,
  and what is known to be missing. Verification levels follow ADR 0005: nothing
  is called *verified* unless it was run.

## Milestone: first playable 2D vertical slice — complete

A window opens, presents rendered frames at the requested presentation mode,
reads the keyboard, runs a fixed-timestep simulation with interpolated
rendering, survives resize and occlusion, and exits cleanly. Applications are
driven by the engine's own runtime: timing and configuration from `mikan-core`,
window lifecycle and input from `mikan-platform`, device/surface/present and
drawing from `mikan-render`.

## What is implemented

| Area | What exists | Coverage |
| --- | --- | --- |
| `mikan-core` | `EngineConfig` + validation (`ConfigError`), `Color`, `Time` with delta clamping and a zero first tick, `FixedTimestep` with bounded catch-up and interpolation alpha, `FrameOutcome`, one-shot `init_logging` | 17 unit tests |
| `mikan-platform` | `App` lifecycle trait, `AppContext`, `InputState` (held state, press/release edges, cursor delta, focus release), `winit` runner with occlusion/suspension-aware waiting and input-edge consumption | 9 unit tests, 1 doc test |
| `mikan-render` | `wgpu` instance/adapter/device/queue/surface, format/alpha/present-mode selection, explicit handling of every swap chain state, resize (including zero-sized), device-lost reporting, instanced quad pipeline, RGBA8 capture readback | 14 unit tests, 1 doc test |
| `games/demo2d` | CLI (`--frames`, `--capture`, `--stats`, `--timeout`, `--help`), fixed-step square with interpolation, corner marker, PPM writer, capture | 16 unit tests |

## Verification: build and tests (this revision, local)

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo check --workspace --all-targets` | pass |
| `cargo test --workspace` | **58 pass, 0 fail** — 16 demo2d, 17 core, 9 platform, 14 render, 2 doc tests |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo build --workspace --release` | pass |

`./scripts/verify.sh` runs all five plus the runtime checks below.

## Verification: runtime (Linux/X11, software Vulkan)

Environment: Xvfb virtual display, Mesa `lavapipe` software Vulkan device
(`llvmpipe (LLVM 20.1.2, 256 bits)`, device type CPU), window 960x600, surface
format `Bgra8UnormSrgb`, present mode `AutoVsync`.

| Check | Command | Observed result |
| --- | --- | --- |
| Frame presentation + capture | `xvfb-run -a ./target/release/demo2d --frames 120 --capture artifacts/frame.ppm --stats` | 120 of 120 frames presented, exit 0, 960x600 PPM written |
| Captured image contents | `python3 scripts/verify_capture.py artifacts/frame.ppm` | verified; background `(63, 69, 80)`, square center `(253, 196, 108)`, marker `(160, 225, 249)` |
| Keyboard movement | `xvfb-run -a bash scripts/verify_input.sh` | holding `D` for 1 s moved the square from x=480 to x=910 (420 px/s, inside the 912 px wall clamp); y unchanged |
| Escape quits | as above | a short tap of `Escape` exited with status 0, without reaching the frame-budget timeout |
| Frame budget timeout | `--frames 100000000 --timeout 1` | exit 1: `presented 675 of 100000000 requested frames within 1s` |
| Unwritable capture path | `--frames 5 --capture /definitely/not/a/dir/f.ppm` | exit 1: `could not write ...: No such file or directory` |
| Invalid argument | `--nope` | exit 1: `unknown argument '--nope'` |
| Help | `--help` | exit 0, usage printed |

Captured pixels are consistent with the configured linear colors converted to
sRGB by the swap chain format, which also confirms the clear color, both quads,
and the readback path agree.

Frame times over those 120 frames: mean 2.53 ms, p50 2.06 ms, p95 4.99 ms,
max 13.99 ms. These describe a **software rasterizer under a virtual display**
with no real vsync; they are not representative of GPU or display performance
and must not be quoted as such.

## Verification: CI

[Run 38731320882](https://github.com/neverseeyouagainnnnn/mikan/actions/runs/38731320882)
for commit `9eaaed8`:

| Job | Result | Duration |
| --- | --- | --- |
| Quality (Ubuntu): fmt, check, tests, clippy, release build | success | 4m21s |
| Headless smoke test (Xvfb + lavapipe) | success | 3m23s |
| Compile check (macos-latest) | success | 27s |
| Compile check (windows-latest) | success | 2m7s |

The run covers `9eaaed8`. The uncommitted changes below — surface alpha-mode
selection, the dropped-input fix, and the new input-verification CI step — were
verified locally but have not run in CI yet; the next push exercises them.

Known CI annotations (non-failing): GitHub reports that `actions/checkout@v4`,
`actions/cache@v4`, and `actions/upload-artifact@v4` target Node.js 20, which is
deprecated on runners. Bumping the action majors is deferred so the currently
green configuration stays pinned until the next push validates the change.

## Verification levels by platform (ADR 0005)

| Platform | Level | Evidence |
| --- | --- | --- |
| Linux / X11 (software Vulkan) | **runtime-verified** | local smoke + input checks, CI smoke job |
| Windows x86-64 | **compile-verified** | CI `cargo check --workspace --all-targets` |
| macOS (arm64) | **compile-verified** | CI `cargo check --workspace --all-targets` |
| Linux / Wayland | not verified | path exists through `winit`; never exercised |
| Hardware GPU (any platform) | not verified | no GPU available in the verification environment |

## Known limitations

- **Graphics scope:** flat colored quads only. No textures, sprites, camera,
  materials, depth buffer, or batching heuristics; one pipeline, one instanced
  draw call per frame.
- **Engine scope:** no ECS, physics, audio, asset pipeline, or 3D. None of these
  are partially implemented or stubbed.
- **Capture:** blocking readback, PPM output only, and restricted to RGBA8/BGRA8
  formats; other formats return `CaptureFormatUnsupported` rather than guessing.
- **Interactive paths that are not automated:** window resize, occlusion,
  minimize/restore, close-button exit, mouse input, and focus-loss release. The
  code paths exist and are unit-tested where they can be (input state, zero-sized
  resize handling), but they have not been exercised end to end.
- **Platform runtime coverage:** only Linux/X11 on a software device has been
  run. Wayland, Windows, and macOS runtime behavior is unverified.
- **Performance:** no GPU measurements exist. The frame times above describe
  `lavapipe` and are labeled as such.

## Uncommitted changes at the time of writing

On top of `9eaaed8` (all verified locally by `./scripts/verify.sh`):

- `crates/mikan-platform/src/runner.rs` — input press/release edges are consumed
  only by frames that ran a fixed update, so a tap can no longer be dropped
  between two simulation steps.
- `crates/mikan-render/src/renderer.rs` — the surface alpha mode is chosen from
  the modes the surface actually advertises (previously `Auto` was configured
  unconditionally).
- `games/demo2d/src/demo.rs` — a capture is written only after the readback
  succeeded, and a failed write is not retried during shutdown.
- `scripts/verify.sh`, `scripts/verify_input.sh` — one-command verification and
  synthetic-input end-to-end checks (new files).
- `.github/workflows/ci.yml` — the smoke job installs `xdotool` plus the explicit
  X11 runtime libraries and runs the input verification.
- `README.md`, `docs/ARCHITECTURE.md`, `docs/DEVELOPMENT.md`, `CONTRIBUTING.md` —
  documentation updated for the license file rename, the input fix, and the new
  scripts.

## Next milestone

Ordered by intended value; each needs its own design note before implementation.

1. **Textures and sprites with a 2D camera** — instance carries an atlas index
   and UV rectangle; the quad pipeline gains a texture bind group; the viewport
   uniform becomes a view/projection matrix.
2. **Engine facade crate (`mikan`)** — owns the platform/render composition that
   `demo2d` currently performs by hand, so applications stop wiring layers.
3. **ECS-lite** — entity storage and systems scheduled on the existing fixed
   timestep.
4. **2D collision** — broad phase plus AABB resolution against the same fixed
   step.
5. **Platform verification** — runtime-verify Windows and macOS (and Wayland)
   per ADR 0005, and record a hardware-GPU measurement.
6. **Asset loading with hot reload** — once there is something to load.

Native C/C++ modules remain out of scope until a measured need meets every
criterion in `docs/ffi.md` and ADR 0004.
