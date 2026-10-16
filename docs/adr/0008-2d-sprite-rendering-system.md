# 0008. PNG assets, sprite-sheet metadata, and a pixel-perfect 2D view

- Status: Accepted
- Date: 2026-10-15

## Context

The next roadmap milestone turns textured sprites into a usable 2D rendering
system: real image files instead of tiles generated in code, named animation
frames instead of hand-picked UV rectangles, the sampling controls pixel art
needs, a pixel-perfect camera mode, and honest performance numbers at scale.
Several decisions inside that milestone are genuinely open:

1. **Where does image decoding live?** PNG decoding is CPU work with no window,
   no device, and no `wgpu` types. Putting it in `mikan-render` would drag the
   graphics stack into every consumer of an image, and putting it in `mikan-core`
   would give the platform-independent vocabulary crate a file format to own.
2. **Which texture encoding do file-loaded images use?** PNG files store sRGB
   bytes. ADR 0006 deliberately made textures linear RGBA8 and deferred sRGB to
   "an image-loading milestone, where the source encoding is known". That
   milestone is this one.
3. **What sprite-sheet format?** Hand-rolled JSON, a bespoke packed binary, or
   an existing tool's export? The engine should not invent a format that every
   artist tool chain then needs an exporter for, and it must validate enough that
   a wrong rectangle is an error, not a wrong frame.
4. **How does rotation fit the pipeline?** A rotated rectangle needs a corner
   transform in the vertex stage. A second pipeline for rotated sprites would be
   amnesia: rotation is data, not a mode.
5. **Nearest versus linear sampling.** Pixel art needs nearest; ordinary art
   needs linear. If the choice were global state it would force pipeline or
   bind-group churn, and if it were per-frame state it would be invisible in
   captures.
6. **What does "pixel-perfect" promise, exactly?** Integer zoom, snapped camera,
   letterboxing, sprite snapping? The answer has to be written down because
   every retro-styled game depends on the exact rule.
7. **How is 10,000 sprites measured without faking it?** A number on one machine
   under one driver is evidence, not a guarantee; the milestone must produce a
   number plus the conditions that produced it, and must check batching instead
   of assuming it.
8. **Does this milestone add C or C++?** The same question ADRs 0006 and 0007
   answered for their milestones. The ADR 0004 criteria — a measured need
   clearing all five requirements in `docs/ffi.md` — still have no measurement
   behind them: the 10,000-sprite benchmark below produces its first real data,
   and it points at rasterization throughput, not at a boundary whose Rust
   expression lacks an obvious native better.

## Decision

1. **A new CPU-only crate: `mikan-assets`.** PNG decoding and sprite-sheet
   parsing live there, depend on `mikan-core` only, and pull in `png`,
   `serde`, and `serde_json` — no `wgpu`, no window, no device. `mikan`
   re-exports the crate (module and common types), and the application performs
   the one glue step: take `Image` pixels into `TextureDescriptor`. The
   dependency direction stays one-way; the render layer never depends on the
   asset layer.
2. **Canonical image representation: RGBA8, top row first, straight alpha, and
   the stored (sRGB) bytes kept.** Every PNG color type is normalized to this
   shape by the `png` crate; limits (`MAX_IMAGE_DIMENSION = 16384`, checked
   allocation) and errors (`ImageError`) belong to Mikan. Textures gain
   `TextureEncoding { Linear, Srgb }`: `Linear` stays the default (procedural
   data, exact backward compatibility with ADR 0006), and file-loaded PNGs are
   uploaded as `Srgb`, so the GPU decodes on sample and the shader's linear
   tint multiply stays correct. Mikan does not CPU-convert pixels; the stored
   bytes are uploaded and the encoding is declared.
3. **Sprite sheets are a documented subset of Aseprite's JSON export**, parsed
   by `SpriteSheet`/`SheetFrame`: named frames with pixel rectangles, optional
   `duration`, optional `pivot` in `[0, 1]`, and `meta.image` resolved relative
   to the JSON file. Validated at load: duplicate names (via a custom serde
   visitor — a derived map would silently drop one), zero-sized sheets or
   frames, rectangles outside the sheet, non-finite or out-of-range pivots, and
   rotated or trimmed frames (rejected explicitly rather than rendered wrong).
   Unknown fields are ignored so exports can grow. The format and its UV
   conversion are documented in `docs/sprite-sheets.md`.
4. **`SheetFrame::uv` insets half a texel on every side.** Skipping a fraction
   of a texel at the frame edge is what stops a linear sampler from reading the
   neighbouring frame's texels — the classic atlas bleeding artifact — and it
   makes a 1×1 frame map to its texel center instead of an inverted rectangle
   (`UvRect::inset` now allows collapse to a point for exactly this case). The
   cost is a sub-texel shift at the very edge when a frame is magnified; at
   1:1 and nearest it is exact. The trade is documented rather than hidden.
5. **Rotation is per-sprite instance data.** `Sprite::with_rotation(radians)`
   stores an angle; the renderer bakes `(cos, sin)` into the instance (stride
   48 → 56 bytes) and the shader rotates each corner around the sprite center.
   One pipeline, one instance layout, one draw path — a rotated sprite is not a
   second kind of sprite.
6. **The sampler mode is part of the batch key, not of the pipeline.** Every
   texture carries two bind groups (linear and nearest);
   `SamplerMode { Linear (default), Nearest }` is a field of `Sprite`, and
   `plan_batches` merges only consecutive sprites sharing both texture *and*
   sampler. Crisp versus smooth is a batching decision, capturable in a
   screenshot, and never forces a pipeline switch.
7. **Batching is observable: `FrameStats { sprites, batches, draw_calls }`**
   from `Renderer::last_frame_stats()`. A draw-call claim in a verification
   script reads the number the renderer actually submitted, so "10,000 sprites,
   1 draw call" is checked, not asserted in prose.
8. **Pixel-perfect mode is core camera math with a written contract.**
   `Camera2d::pixel_perfect(position, viewport, virtual_size)` derives the
   largest integer scale at which the virtual screen fits (`pixel_perfect_scale`,
   at least 1), then snaps the camera so whole world coordinates land on whole
   physical pixels (`snap_to_physical_pixels`). Documented limits: the virtual
   screen is centered to within one physical pixel and there is no letterbox
   fill; a viewport smaller than the virtual grid renders at scale 1; and only
   the camera is snapped — a game that wants sprite movement on the virtual
   grid rounds positions to `1 / scale` itself. The milestone ships the mode
   (`--pixel-perfect WxH` in the demo) and end-to-end verification that every
   rendered edge lies on a multiple of the zoom.
9. **The benchmark is a script with declared conditions, and shows its
   work.** `scripts/bench_sprites.sh` runs the stress scene with vsync off, a
   fixed frame budget, and two window sizes (to separate fill cost from
   per-sprite cost), records per-run logs, and prints a table
   (`scripts/bench_report.py`). 10,000 sprites at one batch and one draw call
   is a verified fact; 60 FPS is a target, not a promise, and the numbers that
   were actually measured — including on a software rasterizer, where 10,000
   sprites is far below 60 FPS — are recorded in `STATUS.md` with the device
   named. No number is quoted without the hardware that produced it.
10. **No native module in this milestone.** The ADR 0004 criteria remain unmet;
    this record exists so the answer to "why is there still no C?" is an
    accepted decision and a measured benchmark, not an omission.

## Consequences

Positive:

- Image decoding is testable without a GPU or a window, and the renderer stays
  a renderer: it accepts pixels, not files.
- Standard PNGs render with the colors their creator saw, because the encoding
  is declared at upload and decoded by the GPU.
- Named frames make sprite sheets usable and validated; the bleeding fix is
  exact where pixel art lives (1:1, nearest) and documented where it trades.
- Rotation, tint, alpha, and both sampler modes are fields on one `Sprite`
  type; the demo's showcase scene pins each feature to a verified pixel.
- Frame stats make batching claims checkable, and the benchmark records its
  conditions next to its numbers.
- Still no new toolchain, no FFI boundary, no `unsafe`; the workspace-wide
  `unsafe_code = "forbid"` constraint stays intact.

Negative:

- Two more dependencies (`png`, `serde_json`, plus `serde`) and one more crate
  to keep coherent — the price of not inventing a decoder.
- `TextureEncoding::Srgb` makes the texture constructor subtly
  encoding-sensitive: uploading sRGB bytes as `Linear` (or the reverse) is a
  visually wrong but valid program. Documented; the demo's asset loader is the
  reference.
- The half-texel inset compresses the outermost half texel of every frame; at
  large magnifications a frame is very slightly smaller than its pixel
  rectangle. Accepted because the alternative is sampling the neighbour.
- Rotated sprites have no separate bounding query; culling and picking must
  account for rotation themselves, as they already do for scaled sprites.
- Sampler-mode changes split batches: a scene alternating linear and nearest
  sprites from one texture pays a draw call per alternation. The showcase
  scene pins this behavior visibly instead of hiding it.
- Pixel-perfect mode does not snap sprites; a game that mixes snapped and
  unsnapped positions gets the shimmer the mode exists to prevent. The rule is
  documented so games can follow it.
- The 10,000-sprite numbers describe `llvmpipe` on one machine and are labeled
  as such; no hardware-GPU measurement exists yet.
