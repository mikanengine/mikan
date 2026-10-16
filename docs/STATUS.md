# Mikan status

- **Date:** 2026-10-16
- **Revision:** `main` @ `2f37c02` ("docs: document the 2D sprite rendering
  milestone"). The Phase 3 change set is committed locally as seven commits
  (`991c17e`..`2f37c02`); **pushes and CI runs still require explicit
  authorization**, so the "Verification: CI" section below records the last
  pushed state, and every claim in this document comes from local runs on this
  machine.
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
exactly. The milestone contains **no C or C++** — ADR 0006 records why the
native-module criteria (ADR 0004, `docs/ffi.md`) are unmet.

## Milestone: the engine facade crate `mikan` — complete

Applications no longer wire the platform and render layers together by hand.
`crates/mikan` provides the `Game` trait, an `Engine` handle carrying the
platform context plus the renderer, and `mikan::run`. Engine and application
failures share one channel; `Engine` hands out `&Renderer` for queries and
texture creation and a `render_frame` call for submission. `demo2d` is a client
that depends on the single `mikan` crate, and the milestone contains **no C or
C++** — ADR 0007 records why the ADR 0004 criteria remain unmet.

## Milestone: 2D sprite rendering system (assets, samplers, pixel-perfect) — complete

Real assets replace code-generated tiles, and the sprite pipeline gains the
controls a 2D renderer needs:

- **`mikan-assets`**, a new CPU-only crate (`mikan-core` + `png` + `serde` +
  `serde_json`, no graphics dependency): `Image` decodes every PNG color type to
  RGBA8 with per-side/allocation limits, and `SpriteSheet`/`SheetFrame` read
  named frames from a documented Aseprite JSON subset with load-time validation
  (duplicate names, zero sizes, out-of-bounds rectangles, non-finite or
  out-of-range pivots, and explicit rejection of rotated/trimmed frames).
  `SheetFrame::uv` insets half a texel per side so linear sampling cannot read a
  neighbouring frame — the atlas-bleeding fix — and a one-texel frame maps to
  its texel center. The format is documented in `docs/sprite-sheets.md`.
- **Textures declare their encoding** (`TextureEncoding::{Linear, Srgb}`):
  procedural data stays exactly as before, and PNG bytes are uploaded as sRGB,
  which the GPU decodes on sample, so the colors a PNG author saw are the colors
  that render.
- **Rotation, tint, and alpha are per-sprite data**: `Sprite::with_rotation`
  bakes `(cos, sin)` into a 56-byte instance; the shader rotates the quad and
  multiplies the sampled texel by the linear tint with straight-alpha blending.
- **Per-sprite sampling** (`SamplerMode::{Linear, Nearest}`) is part of the
  batch key; every texture carries one bind group per mode, so crisp-vs-smooth
  never costs a pipeline switch, and the two modes are visibly different in a
  capture.
- **Draw statistics are observable**: `Renderer::last_frame_stats()` returns
  `FrameStats { sprites, batches, draw_calls }`, which is what the verification
  scripts grep instead of trusting prose.
- **Pixel-perfect mode**: `Camera2d::pixel_perfect` derives the largest integer
  zoom at which a virtual screen fits the window and snaps the camera to the
  physical pixel grid; the contract and its limits (≤1 px centering, no
  letterbox, sprites must be aligned by the game) are recorded in ADR 0008.
- **The demo loads real files**: `games/demo2d/assets/{atlas.png, atlas.json,
  star.png}` are committed, produced deterministically by
  `scripts/generate_assets.py` (stdlib only, `--check` for CI), and loaded
  through `mikan-assets` at startup — a missing, corrupt, or incomplete set
  stops the demo with a readable error before the first frame. Three scenes
  (`classic`, `showcase`, `stress`) and new options (`--scene`, `--sampler`,
  `--pixel-perfect`, `--assets`, `--size`, `--no-vsync`, `--sprites`) exercise
  them.
- **The 10,000-sprite benchmark is a script, not a slogan**:
  `scripts/bench_sprites.sh` measures with vsync off, a fixed frame budget, and
  two window sizes, and the numbers (including the software-rasterizer caveat)
  are recorded below. 10,000 sprites in one batch and one draw call is verified;
  60 FPS is a target, not a claim, and it is not reached on this machine's
  software rasterizer.
- The milestone contains **no C or C++** — ADR 0008 records why the ADR 0004
  criteria remain unmet; its benchmark points at rasterization throughput, not
  at a boundary Rust expresses badly.

## What is implemented

| Area | What exists | Coverage |
| --- | --- | --- |
| `mikan-core` | `EngineConfig` + validation, `Color`, `Time` (clamped delta, zero first tick), `FixedTimestep` (bounded catch-up, interpolation alpha), `FrameOutcome`, `Camera2d` (position, zoom, `world_to_screen`, sanitizing `effective_zoom`, `pixel_space`, `pixel_perfect` integer zoom + snap, `pixel_perfect_scale`, `snap_to_physical_pixels`), `UvRect` (normalized region, collapsible half-texel `inset`), one-shot `init_logging` | 32 unit tests |
| `mikan-assets` | `Image` (PNG → RGBA8, all color types normalized, `MAX_IMAGE_DIMENSION` + checked allocation, `ImageError`), `SpriteSheet`/`SheetFrame` (Aseprite JSON subset, `meta.image` resolution, duplicate detection via a custom serde visitor, geometry/pivot validation, rotated/trimmed rejection, inset `uv`, `center_for_pivot`), `SheetError` | 40 unit tests, 1 doc test |
| `mikan-platform` | `App` lifecycle trait, `AppContext`, `InputState` (held state, press/release edges, cursor delta, focus release), `winit` runner with occlusion/suspension-aware waiting | 9 unit tests, 1 doc test |
| `mikan-render` | `wgpu` instance/adapter/device/queue/surface, format/alpha/present-mode selection, explicit handling of every swap chain state, resize (including zero-sized), device-lost reporting, instanced sprite pipeline (flat or textured, per-sprite rotation, tint, alpha, linear/nearest sampling as a batch key), texture upload with declared encoding (`Linear`/`Srgb`, validated), camera view/projection matrix (including pixel-perfect), `FrameStats { sprites, batches, draw_calls }`, RGBA8 capture readback | 27 unit tests, 1 doc test |
| `mikan` | the `Game` lifecycle over the platform `App` (renderer created before `init`, resizes forwarded before the game sees them, `shutdown` iff `init` ran), the `Engine` handle (`AppContext` + `&Renderer` + `render_frame`), labeled renderer-startup failures, curated re-exports of all four layers | 3 unit tests, 2 doc tests |
| `games/demo2d` | built on the `mikan` facade: CLI (`--frames`, `--capture`, `--stats`, `--timeout`, `--scene`, `--sampler`, `--pixel-perfect`, `--assets`, `--size`, `--camera`, `--zoom`, `--no-vsync`, `--sprites`, `--help`), three scenes, asset loading/upload/validation, PPM writer, frame-time and draw statistics | 32 unit tests |

## Verification: build and tests (this revision, local)

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo check --workspace --all-targets` | pass |
| `cargo test --workspace` | **148 pass, 0 fail** — 143 unit tests (32 demo2d, 3 mikan, 40 mikan-assets, 32 mikan-core, 9 mikan-platform, 27 mikan-render) and 5 doc tests (2 mikan, 1 mikan-assets, 1 mikan-platform, 1 mikan-render) |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo build --workspace --release` | pass |
| `python3 scripts/generate_assets.py --check` | pass: atlas.png, star.png, atlas.json all match the generator |

`./scripts/verify.sh` runs all of the above plus the runtime checks below.

## Verification: runtime (Linux/X11, software Vulkan)

Environment: Xvfb virtual display, Mesa `lavapipe` software Vulkan device
(`llvmpipe (LLVM 20.1.2, 256 bits)`, device type CPU), window 960x600, surface
format `Bgra8UnormSrgb`.

| Check | Command | Observed result |
| --- | --- | --- |
| Frame presentation + capture (classic scene) | `xvfb-run -a ./target/release/demo2d --frames 120 --capture artifacts/frame.ppm --stats` | 120 of 120 frames presented, exit 0; `assets: games/demo2d/assets -> atlas 96x32 (3 frames), star 24x24` |
| Captured image contents | `python3 scripts/verify_capture.py artifacts/frame.ppm` | background `(63, 69, 80)`; player center `(253, 196, 108)` — the exact byte stored in `atlas.png`, which the sRGB upload path now preserves |
| Texture sampling | pixel probes of the same capture | player border `(179, 129, 63)`; marker center `(160, 225, 249)` and border `(89, 149, 188)`: both atlas frames are sampled, not flat colors |
| Draw statistics (classic) | `--stats` line | `sprites 2 | batches 1 | draw calls 1` |
| Keyboard movement | `xvfb-run -a bash scripts/verify_input.sh` | holding `D` for 1 s moved the square to world `[910, 300]` (the 912-pixel wall; earlier runs land anywhere in 730..912 depending on keyup timing); a tapped `Escape` exited 0 before the frame budget |
| Follow camera | `xvfb-run -a bash scripts/verify_camera.sh` | world position `[912, 300]` while the capture shows the square 96x96 px at `[432..527, 252..347]`, centroid `(480.8, 300.8)` — centred, so the view scrolled |
| Zoom | as above (`--camera follow --zoom 2`) | the square renders 192x192 px at `[384..575, 204..395]`, centroid `(482.1, 302.1)` — twice the unzoomed size |
| Showcase scene (rotation, tint, alpha, samplers, PNG alpha) | `xvfb-run -a bash scripts/verify_showcase.sh` | `sprites 8 | batches 4 | draw calls 4`; plain center `(253, 196, 108)`; rotated box corner clear `(63, 69, 80)` while the diamond tip paints `(179, 129, 63)` above the unrotated box; tinted `(136, 156, 108)`; alpha-blended `(174, 138, 93)`; linear checker scanline: 18 distinct grays, 160/160 intermediate; nearest: black/white only; star center `(255, 243, 124)` with backdrop `(80, 97, 149)` through its corners |
| Stress batching (1000 sprites) | `xvfb-run -a bash scripts/verify_stress.sh` | `stress: 1000 sprites, 24x24 cells`; `sprites 1000 | batches 1 | draw calls 1`; 37.0% of pixels are the player tile's warm colors, 38.7% non-clear, 132,983 exactly base color |
| Pixel-perfect mode | `xvfb-run -a bash scripts/verify_pixel_perfect.sh` | `pixel-perfect: virtual 240x150, integer zoom x4`; the player square is exactly 384x384 px at `[288..671, 108..491]` — every edge on a multiple of 4, centre `(480.0, 300.0)` |
| Asset failure modes | `xvfb-run -a bash scripts/verify_assets.sh` | missing directory → `could not read`; corrupt PNG → `could not decode PNG data`; sheet without `player` → `has no frame "player"`; all exit non-zero before presenting a frame |
| Frame budget timeout | `--frames 100000000 --timeout 1` | exit 1: `presented 714 of 100000000 requested frames within 1s` |
| Unwritable capture path | `--frames 5 --capture /definitely/not/a/dir/f.ppm` | exit 1: `could not write '/definitely/not/a/dir/f.ppm': No such file or directory (os error 2)` |
| Invalid argument / option combination | `--nope`; `--zoom 2`; `--sprites 100`; `--camera follow --zoom 2 --pixel-perfect 320x200` | exit 1 with `unknown argument '--nope'`, `'--zoom' requires '--camera' (for example: --camera follow --zoom 2)`, `'--sprites' only applies to '--scene stress'`, `'--zoom' cannot be combined with '--pixel-perfect': the integer zoom is derived from the virtual resolution` |
| Help | `--help` | exit 0, usage printed (all options and scenes listed) |

Captured classic-scene values are identical to the previous milestone's expected
values, except that the player center is now the PNG byte itself
(`(253, 196, 108)` vs `(253, 195, 108)` before), because the sRGB texture path
replaced the CPU conversion.

## Verification: 10,000-sprite benchmark (this machine, software rasterizer)

`xvfb-run -a bash scripts/bench_sprites.sh "100 1000 10000" 300`, recorded in
`artifacts/bench/`; all runs report **1 batch, 1 draw call** for their sprite
count:

| Window | Sprites | frames | mean ms | p50 ms | p95 ms | max ms | mean fps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 320x200 | 100 | 300 | 1.21 | 1.03 | 1.94 | 10.66 | 823.3 |
| 320x200 | 1000 | 300 | 6.05 | 5.54 | 9.01 | 15.27 | 165.3 |
| 320x200 | 10000 | 300 | 60.72 | 55.63 | 90.60 | 141.94 | 16.5 |
| 960x600 | 100 | 300 | 2.60 | 2.26 | 4.63 | 13.14 | 384.6 |
| 960x600 | 1000 | 300 | 11.79 | 10.70 | 21.99 | 32.11 | 84.8 |
| 960x600 | 10000 | 300 | 71.26 | 65.16 | 111.66 | 243.96 | 14.0 |

How to read this, honestly:

- These numbers describe **`llvmpipe` on one machine** with no real vsync;
  they are not representative of GPU performance and must not be quoted as if
  they were.
- Batching is not the bottleneck: every run is one draw call. The 10,000-sprite
  cost is dominated by rasterization: each sprite is 16x16 px, so a frame
  shades ~2.56M sprite fragments regardless of window size (which is why the
  two window sizes differ by ~10 ms at 10,000 sprites, not by 9x), and this
  software rasterizer sustains roughly 36M sprite fragments per second.
- **The 60 FPS target is met up to 1,000 sprites** (84.8 fps at 960x600) **and
  not at 10,000 on this device** (14.0 fps). The target is a measurable goal,
  not a guarantee; a hardware GPU has not been measured (see limitations).

## Verification: CI

**No CI run covers the Phase 3 commits: they are committed locally but not
pushed** (by policy, pushes require explicit authorization). The last pushed
state, `9c3e98b` (documentation for the facade milestone; runtime code
`92398f3`/`431d894`), is green in run
[38753924627](https://github.com/neverseeyouagainnnnn/mikan/actions/runs/38753924627):
fmt/check/tests/clippy/release, the Xvfb smoke job (smoke, input, camera), and
the Windows/macOS compile checks. The repository's `.github/workflows/ci.yml`
and `scripts/verify.sh` now also run the generator check and the showcase,
stress, pixel-perfect, and asset-failure verifications, so the next authorized
push will exercise them in CI.

## Verification levels by platform (ADR 0005)

| Platform | Level | Evidence |
| --- | --- | --- |
| Linux / X11 (software Vulkan) | **runtime-verified** | local smoke, input, camera, showcase, stress, pixel-perfect, and asset-failure checks; CI smoke job for the previous milestone |
| Windows x86-64 | **compile-verified** for the previous milestone | CI `cargo check --workspace --all-targets`; the new crates have not been compile-checked on Windows yet (no push since they were added) |
| macOS (arm64) | **compile-verified** for the previous milestone | as above |
| Linux / Wayland | not verified | path exists through `winit`; never exercised |
| Hardware GPU / Windows / macOS runtime | not verified | no such machine in the verification environment |

## Known limitations

- **Graphics scope:** sprites are rectangles with per-sprite rotation, tint,
  and alpha, sampled linearly or nearest; there are no mipmaps, no texture
  arrays, and no render-to-texture. Textures are single-mip RGBA8 with a
  declared encoding; PNGs are the only decoded format. Batching merges only
  consecutive sprites sharing a texture *and* sampler and never reorders them.
- **Asset scope:** `mikan-assets` decodes PNG files and sheet JSON; there is no
  atlas packing, no hot reload, no background loading, and only the first frame
  of an animated PNG is decoded. `gAMA`/`cHRM` chunks are ignored: file bytes
  are treated as sRGB. Rotated and trimmed sheet frames are rejected.
- **Camera scope:** position and zoom (`pixel_space`, `pixel_perfect`, or a
  follow camera is game code) — no rotation, bounds, smoothing, or parallax.
- **Pixel-perfect scope (ADR 0008):** the virtual screen is centered to within
  one physical pixel with no letterbox fill; only the camera is snapped — a
  game that wants sprites on the virtual grid must round their positions.
- **Facade scope:** one window and one renderer per process; multi-window
  applications must use `mikan_platform::App` directly. `Game::Error` must
  convert from `RenderError`.
- **Engine scope:** no ECS, physics, audio, or 3D. None of these are partially
  implemented or stubbed.
- **Capture:** blocking readback, PPM output only, restricted to RGBA8/BGRA8
  formats.
- **Interactive paths that are not automated:** window resize, occlusion,
  minimize/restore, close-button exit, mouse input, and focus-loss release. The
  code paths exist and are unit-tested where they can be, but they have not
  been exercised end to end.
- **Performance:** all numbers above describe a software rasterizer. No
  hardware-GPU measurement exists, so the 10,000-sprite figure is a measured
  floor of what the pipeline submits (one draw call), not a hardware estimate.

## Milestone changes (committed to `main`)

The facade milestone (`5ffe1c7`, `431d894`, documentation `92398f3`/`9c3e98b`)
plus the Phase 3 change set, committed locally on 2026-10-16 as seven commits
(not yet pushed): `991c17e` (core) → `a7c69f6` (assets) → `7c3c3a1` (render)
→ `2b9d66b` (mikan) → `201190d` (demo2d) → `162f304` (verification) → `2f37c02`
(docs).

In detail:

- **New crate:** `crates/mikan-assets` (`image.rs`, `sheet.rs`, tests) and its
  workspace wiring; `mikan` re-exports it.
- **`mikan-core`:** new `uv.rs` (`UvRect`, moved from `mikan-render`, with a
  collapsible `inset`); `camera.rs` gains `pixel_perfect`,
  `pixel_perfect_scale`, and `snap_to_physical_pixels`.
- **`mikan-render`:** `sprite.rs` gains rotation, `SamplerMode` (two bind
  groups per texture, sampler in the batch key), the 56-byte instance, and
  `FrameStats`; `texture.rs` gains `TextureEncoding`; `renderer.rs` gains
  `last_frame_stats`; `sprite.wgsl` rotates corners.
- **`mikan`:** re-exports of `mikan_assets` and the new types.
- **`demo2d`:** `atlas.rs` deleted; `assets.rs` (file loading + sRGB upload),
  `scene.rs` (three scenes, testable stress layout), extended `cli.rs`,
  reworked `demo.rs` and `main.rs`; committed `games/demo2d/assets/`.
- **Scripts:** `generate_assets.py`, `verify_showcase.{sh,py}`,
  `verify_stress.{sh,py}`, `verify_pixel_perfect.{sh,py}`, `verify_assets.sh`,
  `bench_sprites.sh`, `bench_report.py`; `lib.sh` and `verify.sh` extended.
- **CI:** the generator check and the four new Xvfb verification steps added
  to `.github/workflows/ci.yml`.
- **Docs:** ADR 0008, `docs/sprite-sheets.md`, and refreshed README,
  ARCHITECTURE, DEVELOPMENT, CHANGELOG, and this file.

## Next milestone

Ordered by intended value; each needs its own design note before implementation.

1. **ECS-lite** — entity storage and systems scheduled on the existing fixed
   timestep.
2. **2D collision** — broad phase plus AABB resolution against the same fixed
   step.
3. **Asset pipeline depth** — atlas packing and hot reload on top of
   `mikan-assets`.
4. **Platform and hardware verification** — runtime-verify Windows and macOS
   (and Wayland) per ADR 0005, and record a hardware-GPU run of the sprite
   benchmark next to the `llvmpipe` numbers.

Native C/C++ modules remain out of scope until a measured need meets every
criterion in `docs/ffi.md` and ADR 0004; ADRs 0006, 0007, and 0008 repeat that
finding for their milestones rather than leaving it implicit.
