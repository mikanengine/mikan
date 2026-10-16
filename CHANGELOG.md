# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
The engine is pre-1.0: minor versions may change APIs, and those changes are
listed here.

## [Unreleased]

### Added

- `mikan-assets`: a CPU-only crate (`png`, `serde`, `serde_json`; no graphics
  dependency) providing `Image` (PNG decoded to RGBA8, every color type
  normalized, per-side and allocation limits, descriptive `ImageError`s) and
  `SpriteSheet`/`SheetFrame` (named frames from a documented Aseprite JSON
  subset, with `meta.image` resolved relative to the document, duplicate-name
  detection, and validation of sizes, rectangles, pivots, and unsupported
  rotated/trimmed frames). Frames convert to inset `UvRect`s so linear
  sampling cannot bleed between atlas frames; the format and the trade are
  documented in `docs/sprite-sheets.md`.
- `mikan-core`: `UvRect` (moved from `mikan-render`; `inset` now allows
  collapsing to a point, which is what a one-texel frame needs), and
  pixel-perfect camera support: `Camera2d::pixel_perfect`,
  `pixel_perfect_scale`, and `snap_to_physical_pixels`, so one virtual pixel
  maps to a whole number of physical pixels and the camera sits on the
  physical grid.
- `mikan-render`: per-sprite rotation (`Sprite::with_rotation`, baked as
  `(cos, sin)` into a 56-byte instance), per-sprite sampling
  (`SamplerMode::{Linear, Nearest}` as part of the batch key; every texture
  carries one bind group per mode, so filtering never costs a pipeline
  switch), `TextureEncoding::{Linear, Srgb}` on texture upload (PNGs keep
  their stored sRGB bytes and the GPU decodes on sample), and
  `Renderer::last_frame_stats()` returning `FrameStats { sprites, batches,
  draw_calls }` for the last submitted frame.
- `mikan`: re-exports of `mikan_assets` and the new core/render types
  (`Image`, `ImageError`, `SheetError`, `SheetFrame`, `SpriteSheet`,
  `UvRect`, `SamplerMode`, `TextureEncoding`, `FrameStats`).
- `demo2d`: three scenes — `classic`, `showcase` (rotation, tint, alpha,
  nearest vs linear, and PNG alpha at fixed verified positions), and `stress`
  (N sprites from one texture) — plus `--scene`, `--sampler`,
  `--pixel-perfect WxH`, `--assets DIR`, `--size WxH`, `--no-vsync`, and
  `--sprites N`; assets are committed PNGs loaded and validated at startup;
  `--stats` now also prints the last frame's sprites/batches/draw calls.
- Asset generator and verification: `scripts/generate_assets.py` (stdlib-only,
  deterministic, `--check` for CI), `scripts/verify_showcase.sh`,
  `scripts/verify_stress.sh`, `scripts/verify_pixel_perfect.sh`,
  `scripts/verify_assets.sh`, and an honest benchmark harness
  (`scripts/bench_sprites.sh` + `scripts/bench_report.py`); `scripts/verify.sh`
  and the CI smoke job run the new checks.
- Documentation: ADR 0008 (assets, samplers, rotation, pixel-perfect
  contract, benchmark honesty, and no native module in this milestone) and
  `docs/sprite-sheets.md`; refreshed README, ARCHITECTURE, DEVELOPMENT,
  STATUS, and this changelog.
- Workspace foundation: `mikan-core`, `mikan-platform`, `mikan-render`, and the
  `demo2d` application, with a pinned toolchain, shared lint configuration, and a
  committed lockfile.
- `mikan-core`: validated `EngineConfig`, clamped frame `Time`, `FixedTimestep`
  with a bounded step budget and interpolation alpha, `FrameOutcome`, `Color`,
  `Camera2d` (position, zoom, `world_to_screen`, pixel-space helper), and
  one-shot logging initialization.
- `mikan-platform`: the `App` lifecycle trait, `AppContext` (window handle, input,
  config, timing, exit requests), `InputState` with held/edge queries and
  focus-release handling, and a `winit`-based event loop runner with an
  occlusion/suspension aware wait policy.
- `mikan-render`: `wgpu` device and surface management with explicit handling of
  every swap chain state (`Success`, `Suboptimal`, `Timeout`, `Occluded`,
  `Outdated`, `Lost`, `Validation`), resize handling for zero-sized windows,
  device-lost reporting, an instanced sprite pipeline (flat colored or textured
  rectangles, atlas-ready UV regions, one draw call per consecutive texture run,
  painter order equals submission order), validated linear RGBA8 texture upload,
  a view/projection camera matrix, and optional single-frame RGBA8 readback.
- `mikan`: the engine facade an application depends on — a `Game` lifecycle
  trait that mirrors `App` hook for hook, an `Engine` handle (platform context
  plus renderer, with frame submission as the only mutable path),
  `mikan::run` with renderer creation, resize forwarding, and labeled
  renderer-startup failures, `Game::Error: From<RenderError>` error
  translation, and curated re-exports of the three layers.
- `demo2d`: a playable demo with fixed-timestep movement (WASD and arrow keys),
  delta-time-independent speed, Escape and window-close exit, `--camera`/`--zoom`
  options alongside `--frames`/`--capture`/`--stats`/`--timeout`, and a PPM
  writer for captured frames. Built on the `mikan` facade and depending on no
  other engine crate.
- Verification: `scripts/verify.sh` runs every gate plus the headless smoke
  test, `scripts/verify_input.sh` injects real X11 key events, and
  `scripts/verify_camera.sh` proves the follow camera keeps the moving player
  centered and that `--zoom` scales the rendered square.
- Documentation: architecture guide, development guide, FFI policy, ADRs
  (0006, 0007, and 0008 record why the sprites/camera, facade, and
  sprite-rendering milestones contain no native module), status report,
  contributing guide, and this changelog.
- CI: formatting, lint, test, and release build gates; a headless smoke test on
  `Xvfb` with a software Vulkan device that verifies a captured frame; and
  compile checks on Windows and macOS.

### Changed

- `demo2d` no longer generates its atlas in code: it loads `atlas.png`,
  `atlas.json`, and `star.png` from `games/demo2d/assets` and uploads them as
  sRGB textures.
- `UvRect` now lives in `mikan-core` and is re-exported by `mikan-render`, so
  the asset layer can produce UVs without depending on the render layer.

### Removed

- `games/demo2d/src/atlas.rs` — the procedural two-tile atlas, replaced by
  committed, generated PNG assets.

[Unreleased]: https://github.com/neverseeyouagainnnnn/mikan/commits/main
