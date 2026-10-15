# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
The engine is pre-1.0: minor versions may change APIs, and those changes are
listed here.

## [Unreleased]

### Added

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
  delta-time-independent speed, Escape and window-close exit, a procedural
  texture atlas (no image-loading dependency), `--camera`/`--zoom` options
  alongside `--frames`/`--capture`/`--stats`/`--timeout`, and a PPM writer for
  captured frames. Built on the `mikan` facade and depending on no other engine
  crate.
- Verification: `scripts/verify.sh` runs every gate plus the headless smoke
  test, `scripts/verify_input.sh` injects real X11 key events, and
  `scripts/verify_camera.sh` proves the follow camera keeps the moving player
  centered and that `--zoom` scales the rendered square.
- Documentation: architecture guide, development guide, FFI policy, seven ADRs
  (0006 and 0007 record why the sprites/camera and facade milestones contain no
  native module), status report, contributing guide, and this changelog.
- CI: formatting, lint, test, and release build gates; a headless smoke test on
  `Xvfb` with a software Vulkan device that verifies a captured frame; and
  compile checks on Windows and macOS.

[Unreleased]: https://github.com/neverseeyouagainnnnn/mikan/commits/main
