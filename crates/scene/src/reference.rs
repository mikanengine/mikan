//! An independent Rust transcription of the native transform kernel.
//!
//! The engine does **not** execute this code: world transforms are composed by
//! the C kernel in `crates/scene-sys/src/transform.c`. This module exists so
//! the test suite can cross-check the native result against a second,
//! independent implementation written from the same specification
//! (`crates/scene-sys/src/mikan_transform.h`), bit for bit.
//!
//! The grouping of every product and sum below is part of the contract, not
//! an implementation detail: `a * b + c * d` in one order can round
//! differently than in another, and the kernel promises the *exact* sequence
//! of operations documented in the header. Do not "simplify" this file or the
//! C file without changing the header and the tests together.

use mikan_scene_sys::Transform2dRepr;

/// Composes a parent world transform with a local transform
/// (`out = parent ∘ local`), exactly as the native kernel does.
///
/// See `crates/scene-sys/src/transform.c` for the reference grouping, which
/// this function mirrors operation for operation.
#[must_use]
pub fn compose(parent: &Transform2dRepr, local: &Transform2dRepr) -> Transform2dRepr {
    Transform2dRepr {
        translation_x: parent.translation_x
            + (parent.cos_rotation * (parent.scale_x * local.translation_x)
                - parent.sin_rotation * (parent.scale_y * local.translation_y)),
        translation_y: parent.translation_y
            + (parent.sin_rotation * (parent.scale_x * local.translation_x)
                + parent.cos_rotation * (parent.scale_y * local.translation_y)),
        rotation: parent.rotation + local.rotation,
        scale_x: parent.scale_x * local.scale_x,
        scale_y: parent.scale_y * local.scale_y,
        cos_rotation: parent.cos_rotation * local.cos_rotation
            - parent.sin_rotation * local.sin_rotation,
        sin_rotation: parent.sin_rotation * local.cos_rotation
            + parent.cos_rotation * local.sin_rotation,
    }
}

/// The bit pattern of a transform value, for tests that must distinguish
/// "equal" from "identical" (`-0.0` vs `0.0`, or two different roundings).
#[must_use]
pub fn bits(transform: &Transform2dRepr) -> [u32; 7] {
    [
        transform.translation_x.to_bits(),
        transform.translation_y.to_bits(),
        transform.rotation.to_bits(),
        transform.scale_x.to_bits(),
        transform.scale_y.to_bits(),
        transform.cos_rotation.to_bits(),
        transform.sin_rotation.to_bits(),
    ]
}
