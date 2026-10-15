# 0002. wgpu for rendering and winit for windowing

- Status: Accepted
- Date: 2026-10-15

## Context

The engine needs to draw to a window on Windows, Linux, and macOS now, with
Linux as the first runtime-verified platform. That requires two decisions that
are closely related: a graphics API abstraction and a windowing/event handling
library.

Candidates for graphics:

- **Raw Vulkan/Metal/DX12** — no third-party dependency, full control, and
  three completely separate implementations to write and maintain, plus `unsafe`
  throughout and large amounts of boilerplate before a triangle appears.
- **OpenGL via a loader (`glow`)** — simplest to start; deprecated on macOS,
  weak on modern features, and a dead end for compute or modern synchronization.
- **`gfx-hal`** — the former low-level HAL; effectively superseded by `wgpu`.
- **`wgpu`** — a single safe API over Vulkan, Metal, DX12, and GLES; maintained
  by the same organization that defines the WebGPU standard; shaders written in
  WGSL; validation built in.

Candidates for windowing and input:

- **`SDL3` / `GLFW` via bindings** — mature and feature-rich; a C dependency in
  the build, a C-style API in safe-Rust clothing, and their own event loops to
  adapt.
- **`winit`** — the Rust-native windowing library; the de-facto standard,
  integrated with `raw-window-handle`, which is also what `wgpu` consumes for
  surface creation.

The pairing matters: `wgpu` and `winit` are designed to cooperate through
`raw-window-handle`, which keeps the two layers decoupled rather than forcing the
renderer to depend on the windowing library's types.

## Decision

Use `wgpu` for all rendering, and `winit` for window creation and event
handling. Both are pinned in the workspace manifest and upgraded deliberately,
not automatically.

Constraints that follow from this decision and are enforced in code:

- The renderer restricts itself to the **WebGPU default feature set**
  (`Features::empty()`, `Limits::default()`), which keeps it portable across
  backends and avoids depending on vendor-specific extensions.
- Shaders are written in **WGSL** and compiled by `wgpu` at runtime. No
  platform-specific shader languages are used.
- Swap chain outcomes are handled explicitly, because `wgpu` exposes them
  (`CurrentSurfaceTexture`), and any abstraction that hides them would hide the
  conditions that actually break games on real hardware.
- Only `mikan-render` may depend on `wgpu`; only `mikan-platform` may depend on
  `winit`. Applications depend on the engine's APIs, not directly on either
  library, except to name types the engine re-exports.

## Consequences

Positive:

- One rendering implementation covers Vulkan, Metal, DX12, and GLES, so
  cross-platform work is configuration rather than new backends.
- Validation runs in development and reports precise errors instead of producing
  a black screen.
- Software fallback (Mesa's `lavapipe`) works through the same code path, which
  makes headless CI verification possible.
- Windowing and rendering stay decoupled: the renderer consumes
  `raw-window-handle`, not `winit` types (ADR 0003).

Negative:

- `wgpu` is a large dependency with its own upgrade cadence; API breakage between
  major versions is expected, and upgrades are treated as scheduled work.
- Features that a backend exposes but WebGPU does not (some ray tracing paths,
  vendor extensions) are unavailable without opting out of portability.
- WGSL's ecosystem is younger than HLSL/GLSL, so some tooling and examples are
  harder to find.
- `wgpu`'s abstraction has a small cost relative to a hand-written backend, which
  is accepted in exchange for three fewer implementations.
