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
  and one-shot logging initialization.
- `mikan-platform`: the `App` lifecycle trait, `AppContext` (window handle, input,
  config, timing, exit requests), `InputState` with held/edge queries and
  focus-release handling, and a `winit`-based event loop runner with an
  occlusion/suspension aware wait policy.
- `mikan-render`: `wgpu` device and surface management with explicit handling of
  every swap chain state (`Success`, `Suboptimal`, `Timeout`, `Occluded`,
  `Outdated`, `Lost`, `Validation`), resize handling for zero-sized windows,
  device-lost reporting, an instanced screen-space quad pipeline, and optional
  single-frame RGBA8 readback.
- `demo2d`: a playable demo with fixed-timestep movement (WASD and arrow keys),
  delta-time-independent speed, resize handling, Escape and window-close exit,
  `--frames`/`--capture`/`--stats`/`--timeout` command-line options, and a PPM
  writer for captured frames.
- Documentation: architecture guide, development guide, FFI policy, five ADRs,
  status report, contributing guide, and this changelog.
- CI: formatting, lint, test, and release build gates; a headless smoke test on
  `Xvfb` with a software Vulkan device that verifies a captured frame; and
  compile checks on Windows and macOS.

[Unreleased]: https://github.com/neverseeyouagainnnnn/mikan/commits/main
