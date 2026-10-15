# Mikan status

- **Date:** 2026-10-15
- **Revision:** `main` @ `1de2e22` ("docs: document the sprites and camera
  milestone"), pushed to `origin/main`; the refresh of this status text is the
  commit that contains this line. The milestone commits are listed under
  [Milestone changes](#milestone-changes-committed-to-main).
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

## Milestone: textured sprites and a minimal 2D camera — complete

`Quad2d` was replaced by `Sprite`: a rectangle in world space that is either
flat-colored or samples a texture through an atlas-ready UV rectangle. Textures
are uploaded from linear RGBA8 data after validation, drawn with one instanced
draw call per consecutive run of sprites sharing a texture (painter order equals
submission order, sprites are never reordered), and rendered through an optional
`Camera2d` (position and zoom, defined in `mikan-core`, applied in
`mikan-render` as a view/projection matrix). A frame without a camera renders in
pixel space through `Camera2d::pixel_space`, reproducing the previous milestone
exactly. The demo generates a two-tile atlas in code, adds `--camera <none|follow>`
and `--zoom <factor>`, and the milestone contains **no C or C++** — ADR 0006
records why the native-module criteria (ADR 0004, `docs/ffi.md`) are unmet.

## What is implemented

| Area | What exists | Coverage |
| --- | --- | --- |
| `mikan-core` | `EngineConfig` + validation (`ConfigError`), `Color`, `Time` with delta clamping and a zero first tick, `FixedTimestep` with bounded catch-up and interpolation alpha, `FrameOutcome`, `Camera2d` (position, zoom, `world_to_screen`, sanitizing `effective_zoom`, `pixel_space`), one-shot `init_logging` | 22 unit tests |
| `mikan-platform` | `App` lifecycle trait, `AppContext`, `InputState` (held state, press/release edges, cursor delta, focus release), `winit` runner with occlusion/suspension-aware waiting and input-edge consumption | 9 unit tests, 1 doc test |
| `mikan-render` | `wgpu` instance/adapter/device/queue/surface, format/alpha/present-mode selection, explicit handling of every swap chain state, resize (including zero-sized), device-lost reporting, validated linear RGBA8 texture upload, instanced sprite pipeline (flat or textured, UV rectangles, per-run batching), camera view/projection matrix, RGBA8 capture readback | 26 unit tests, 1 doc test |
| `games/demo2d` | CLI (`--frames`, `--capture`, `--stats`, `--timeout`, `--camera`, `--zoom`, `--help`), fixed-step square with interpolation, procedural two-tile atlas, corner marker, PPM writer, capture | 23 unit tests |

## Verification: build and tests (this revision, local)

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo check --workspace --all-targets` | pass |
| `cargo test --workspace` | **82 pass, 0 fail** — 23 demo2d, 22 core, 9 platform, 26 render, 2 doc tests |
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
| Captured image contents | `python3 scripts/verify_capture.py artifacts/frame.ppm` | verified; background `(63, 69, 80)`, square center `(253, 195, 108)` |
| Texture sampling | pixel probe of the same capture | square border `(179, 129, 64)` differs from its fill, marker tile center `(159, 224, 249)` and border `(90, 149, 188)` are the other tile: both atlas tiles are sampled, not flat colors |
| Keyboard movement | `xvfb-run -a bash scripts/verify_input.sh` | holding `D` for 1 s moved the square from x=480 to x=912 (420 px/s, the right wall clamp); y unchanged |
| Escape quits | as above | a short tap of `Escape` exited with status 0, without reaching the frame-budget timeout |
| Follow camera | `xvfb-run -a bash scripts/verify_camera.sh` | after the same 1 s hold the square's **world** position was x=912, while the capture shows it 95x96 px at `[432..526, 252..347]`, centroid `(480.2, 300.7)` — centred, so the view scrolled with it |
| Zoom | as above (`--camera follow --zoom 2`) | the square renders 191x192 px centred (`[384..574, 204..395]`, centroid `(481.5, 302.0)`) — twice the 96 px of the unzoomed path |
| Frame budget timeout | `--frames 100000000 --timeout 1` | exit 1: `presented 640 of 100000000 requested frames within 1s` |
| Unwritable capture path | `--frames 5 --capture /definitely/not/a/dir/f.ppm` | exit 1: `could not write ...: No such file or directory` |
| Invalid argument | `--nope` | exit 1: `unknown argument '--nope'` |
| Zoom without camera | `--zoom 2` | exit 1: `'--zoom' requires '--camera'` |
| Help | `--help` | exit 0, usage printed (camera options listed) |

Captured pixels are consistent with the configured linear colors and texels
converted to sRGB by the swap chain format, which confirms the clear color,
texture upload and sampling, and the readback path agree.

Frame times over those 120 frames (one run; they vary between runs): mean
2.03 ms, p50 1.70 ms, p95 3.35 ms, max 12.50 ms. These describe a **software
rasterizer under a virtual display** with no real vsync; they are not
representative of GPU or display performance and must not be quoted as such.

## Verification: CI

Every commit on `main` has a green CI run. Latest:
[38747734131](https://github.com/neverseeyouagainnnnn/mikan/actions/runs/38747734131)
for commit `1de2e22` — the sprites, textures, and camera milestone:

| Job | Result | Duration |
| --- | --- | --- |
| Quality (Ubuntu): fmt, check, tests, clippy, release build | success | 1m25s |
| Headless smoke test (Xvfb + lavapipe): smoke, input, camera, artifacts | success | 2m57s |
| Compile check (macos-latest) | success | 30s |
| Compile check (windows-latest) | success | 1m49s |

[Run 38734036511](https://github.com/neverseeyouagainnnnn/mikan/actions/runs/38734036511)
covers `53afe21` (input verification end to end) and
[38731320882](https://github.com/neverseeyouagainnnnn/mikan/actions/runs/38731320882)
covers `9eaaed8`; both are green, so the milestone is runtime-verified in CI and
not only locally.

Known CI annotations (non-failing): GitHub reports that `actions/checkout@v4`,
`actions/cache@v4`, and `actions/upload-artifact@v4` target Node.js 20, which is
deprecated on runners. Bumping the action majors is deferred so the currently
green configuration stays pinned until the next push validates the change.

## Verification levels by platform (ADR 0005)

| Platform | Level | Evidence |
| --- | --- | --- |
| Linux / X11 (software Vulkan) | **runtime-verified** | local smoke + input + camera checks, CI smoke job |
| Windows x86-64 | **compile-verified** | CI `cargo check --workspace --all-targets` |
| macOS (arm64) | **compile-verified** | CI `cargo check --workspace --all-targets` |
| Linux / Wayland | not verified | path exists through `winit`; never exercised |
| Hardware GPU (any platform) | not verified | no GPU available in the verification environment |

## Known limitations

- **Graphics scope:** sprites are axis-aligned rectangles with no rotation;
  textures are linear RGBA8 with a single mip, uploaded from memory by the
  application (no image loading, atlas packing, or sRGB textures); batching
  merges only consecutive sprites sharing a texture and never reorders them to
  reduce draw calls; one pipeline; no depth buffer, materials, or 3D.
- **Camera scope:** position and zoom only — no rotation, bounds, smoothing, or
  parallax. Games implement those on top of `Camera2d`.
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

## Milestone changes (committed to `main`)

Three commits on top of `53afe21`:

- `128ab33` — **render: replace the quad pipeline with textured sprites and a
  camera**: `Camera2d` in `mikan-core`; `Sprite`, `UvRect`, the 48-byte
  instance, `plan_batches` (consecutive same-texture runs), the
  view/projection matrix, and the fallback-texture pipeline in `mikan-render`
  (`quad.rs`/`quad.wgsl` removed); validated linear RGBA8 texture upload; the
  demo's procedural atlas and `--camera`/`--zoom`.
- `476cf13` — **verification: check the 2D camera end to end in scripts and CI**:
  `scripts/lib.sh` (shared X11 helpers, PID-scoped cleanup), `scripts/ppm.py`
  (shared PPM parser), `scripts/verify_camera.sh`/`.py` (follow and zoom
  checks), and the camera-verification step in the CI smoke job.
- `1de2e22` — **docs: document the sprites and camera milestone**: ADR 0006
  (decisions and the no-native-module rationale) plus README, ARCHITECTURE,
  DEVELOPMENT, STATUS, CONTRIBUTING, and CHANGELOG updates.

All three ran through CI in run 38747734131 (above); the local
`./scripts/verify.sh` runs before the push produced the evidence quoted in the
runtime section.

## Next milestone

Ordered by intended value; each needs its own design note before implementation.

1. **Engine facade crate (`mikan`)** — owns the platform/render composition that
   `demo2d` currently performs by hand, so applications stop wiring layers.
2. **ECS-lite** — entity storage and systems scheduled on the existing fixed
   timestep.
3. **2D collision** — broad phase plus AABB resolution against the same fixed
   step.
4. **Asset loading with hot reload** — once there is something to load; image
   decoding plugs in front of `Renderer::create_texture`.
5. **Platform verification** — runtime-verify Windows and macOS (and Wayland)
   per ADR 0005, and record a hardware-GPU measurement.

Native C/C++ modules remain out of scope until a measured need meets every
criterion in `docs/ffi.md` and ADR 0004; ADR 0006 repeats that finding for this
milestone rather than leaving it implicit.
