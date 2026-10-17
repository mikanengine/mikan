/*
 * Mikan 2D transform kernel — the low-level numeric layer of the scene system.
 *
 * This header is the contract; the Rust mirror lives in
 * crates/scene-sys/src/lib.rs, the C++ scene graph calls these functions
 * (crates/scene-sys/src/scene_graph.cpp), and the test suite cross-checks the
 * kernel against an independent Rust implementation (crates/scene/src/reference.rs)
 * bit for bit.
 *
 * Semantics of mikan_transform2d_compose:
 *
 *   - It composes a parent world transform with a local transform:
 *     out = parent ∘ local, so a child's local translation is rotated and
 *     scaled by the parent before the parent's translation is added. This is
 *     the standard 2D TRS (translation, rotation, scale) composition.
 *   - Rotation is stored three ways in the struct, and the invariant is that
 *     (cos_rotation, sin_rotation) == (cos(rotation), sin(rotation)) and that
 *     the pair is unit length. The C side never computes trigonometry: the
 *     Rust wrapper caches the pair once per value, and the kernel keeps it
 *     exact through the composition identities
 *     cos(a+b) = cos a cos b − sin a sin b and sin(a+b) = sin a cos b + cos a sin b.
 *   - All arithmetic is plain f32 multiply/add in a fixed order (mirrored
 *     exactly by the Rust reference), so the C and Rust results are
 *     bit-identical on one target. No FMA contraction is allowed for the
 *     kernel's sources (the build disables it for ISO C).
 *   - Non-finite inputs are not special-cased: unlike collision, a transform
 *     has no "separated" answer, so the kernel computes IEEE arithmetic and a
 *     non-finite input yields a non-finite output. Gameplay code that cares
 *     must validate before it composes.
 *   - Thread safety: the function is pure — it reads its two inputs, writes
 *     `out`, and retains no pointer. Distinct calls may run concurrently;
 *     a single call must not alias `out` with `parent` or `local` (the Rust
 *     wrapper enforces that with the borrow checker).
 */
#ifndef MIKAN_TRANSFORM_H
#define MIKAN_TRANSFORM_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct mikan_transform2d {
    float translation_x;
    float translation_y;
    /* Rotation in radians; the source of truth, summed exactly. */
    float rotation;
    float scale_x;
    float scale_y;
    /* Cached cos(rotation), sin(rotation); see the invariant above. */
    float cos_rotation;
    float sin_rotation;
} mikan_transform2d;

void mikan_transform2d_compose(const mikan_transform2d *parent,
                               const mikan_transform2d *local,
                               mikan_transform2d *out);

#ifdef __cplusplus
}
#endif

#endif /* MIKAN_TRANSFORM_H */
