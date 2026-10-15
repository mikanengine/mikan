# 0006. Textured sprites, run batching, and a minimal 2D camera

- Status: Accepted
- Date: 2026-10-15

## Context

The next roadmap milestone is texture and sprite support with a minimal 2D
camera, described in `STATUS.md` as "instance carries an atlas index and UV
rectangle; the quad pipeline gains a texture bind group; the viewport uniform
becomes a view/projection matrix". Unlike the windowed vertical slice, which
was prescribed by the architecture, several decisions inside this milestone are
genuinely open:

1. **Two rectangle types or one?** The engine has `Quad2d` (flat color). A
   textured sprite is the same rectangle plus a texture and a UV region.
   Keeping both invites two pipelines and two draw paths for one concept.
2. **Texture encoding.** `wgpu` offers sRGB and linear formats. `Color` in
   `mikan-core` is linear, and the shader multiplies a texel by the sprite
   color; mixing encodings would silently double-encode one of them.
3. **Batching policy.** Sprites referencing different textures need different
   bind groups. The options range from "one draw call per sprite" to a full
   sprite sorter, which changes painter order.
4. **Where the camera lives.** The camera is math, not graphics API; but only
   the render layer needs the matrix form of it.
5. **Does this milestone introduce C or C++?** The repository's stated
   direction ("C at the foundation, C++ where it counts") could be read as an
   instruction to start a native module now. ADR 0004 requires a *measured*
   need that clears all five criteria in `docs/ffi.md` first, and the user
   direction for this work explicitly forbids adding C or C++ merely to
   demonstrate three-language usage.

There is no profiling data for any Mikan subsystem (no benchmark suite exists
yet), no third-party native library is being integrated, and nothing about 2D
sprites and a camera stresses a boundary that Rust expresses worse than C or
C++. Both candidate native components — an allocator arena and a batch builder
— are hypothetical, and scaffolding either one now is exactly what ADR 0004
was written to prevent.

## Decision

1. **One sprite type.** `Quad2d` is replaced by `Sprite`, which carries
   `center`, `size`, `color`, an optional `&Texture`, and a `UvRect`. A sprite
   without a texture samples a shared 1×1 white fallback, so flat and textured
   rectangles share one pipeline, one instance layout, and one shader. The
   instance grows from 32 to 48 bytes (adds the UV rectangle) and its layout
   stays unit-tested against the shader.
2. **Textures are linear RGBA8** (`Rgba8Unorm`, single mip, tightly packed),
   uploaded by `Renderer::create_texture`. Linear values match `Color`, so the
   shader's `texel * color` multiply is uniform across the engine; an sRGB
   format belongs to an image-loading milestone, where the source encoding is
   known. Descriptors are validated (non-zero, within the device limit, exact
   data length) before any GPU resource is created. A texture is usable only
   with the `Renderer` that created it, enforced by taking `&Texture` from that
   renderer's frames.
3. **Painter order equals submission order, always.** Batching never reorders
   sprites: `plan_batches` merges only *consecutive* sprites that share a
   texture, and the renderer issues one instanced draw call per run. A run of
   solid sprites is a run through the same fallback texture. This is
   deliberately the simplest rule that keeps correctness and cuts draw calls
   for the common atlas case; sorting to reduce draw calls is future work, not
   a silent behavior change.
4. **The camera is core math.** `Camera2d` (position, zoom) lives in
   `mikan-core`, with `world_to_screen` as the reference transformation and a
   sanitizing `effective_zoom` (invalid zoom renders as 1:1 instead of NaN).
   `mikan-render` converts it to a column-major view/projection matrix; a frame
   without a camera uses `Camera2d::pixel_space`, which reproduces the Phase 1
   screen-space behavior exactly, so both paths share one matrix code path and
   one shader.
5. **No native module in this milestone.** The C/C++ criteria in ADR 0004 and
   `docs/ffi.md` are unmet, so this milestone is Rust-only. The record of the
   reasoning lives here so that "why is there still no C?" has an answer that
   is not an omission.

## Consequences

Positive:

- One sprite concept for flat rectangles, textured quads, and atlas regions;
  applications do not pick a pipeline.
- One shader and one instance layout; the layout test keeps them in sync.
- A frame with no camera renders exactly as it did before this milestone,
  which keeps the Phase 1 smoke and input checks valid as regressions.
- Draw calls scale with texture changes, not sprite count, for the atlas case
  the milestone targets.
- `Camera2d` has no graphics dependency, so gameplay (culling, picking) can
  use it, and its math is unit-tested without a GPU.
- No new toolchain, no FFI boundary, no `unsafe`; the workspace-wide
  `unsafe_code = "forbid"` constraint stays intact.

Negative:

- API break: `Quad2d` disappears and `Frame` changes shape. Pre-1.0 this is
  allowed; the changelog records it.
- A linear texture format is not what most image files store; loading sRGB
  images will need an explicit conversion step (a future decision), and the
  current format cannot represent one.
- Consecutive-run batching is weaker than a sprite sorter: interleaving sprites
  from many textures produces many draw calls. Accepted for now because
  sorting would silently change painter order.
- No texture reuse validation beyond "created by this renderer"; uploading the
  same atlas twice wastes VRAM. Accepted until an asset pipeline exists.
- The camera has no rotation, bounds, or smoothing; games implement those on
  top of `Camera2d`.
