//! 2D collision detection for Mikan: axis-aligned boxes, a uniform-grid broad
//! phase, and the safe face of the C narrow-phase kernel.
//!
//! The crate is CPU-only and knows nothing about the ECS, the window, or the
//! renderer; the demo integrates it. Three layers live here:
//!
//! - [`Broadphase`] — a uniform grid that turns a slice of boxes into
//!   deduplicated candidate [`Pair`]s, deterministically (its output is
//!   sorted and independent of hash-map order).
//! - [`contacts`] — one batch call into the C kernel
//!   (`crates/collision-sys`), writing one [`Contact`] per pair.
//! - [`reference`] — the Rust implementation of the same contact math,
//!   kept as the test oracle and the benchmark baseline. The test suite
//!   cross-checks it against the C kernel on fixed and randomized cases.
//!
//! # Semantics
//!
//! Overlap means `depth > 0`; boxes that merely touch are separated, so a
//! resting body does not jitter. The normal points from the pair's `first`
//! box toward its `second` box — that is, translating `second` by
//! `normal * depth` separates the pair. Resolution policy (which body moves)
//! is deliberately not decided here; that belongs to the simulation layer.
//!
//! The guide is `docs/collision.md`; the decision record, including why the
//! narrow-phase kernel is C and what that boundary promises, is ADR 0010.
//!
//! ```
//! use mikan_collision::{Aabb, Broadphase, Contact, Pair, contacts};
//!
//! let bounds = [
//!     Aabb::from_center_size([0.0, 0.0], [10.0, 10.0]),
//!     Aabb::from_center_size([6.0, 1.0], [10.0, 10.0]),
//!     Aabb::from_center_size([500.0, 0.0], [10.0, 10.0]),
//! ];
//!
//! let mut broadphase = Broadphase::new(16.0);
//! broadphase.rebuild(&bounds);
//! let mut pairs: Vec<Pair> = Vec::new();
//! broadphase.pairs(&mut pairs);
//! assert_eq!(pairs.len(), 1, "only the first two boxes are near each other");
//!
//! let mut out = vec![Contact { normal: [0.0, 0.0], depth: 0.0 }; pairs.len()];
//! contacts(&bounds, &pairs, &mut out);
//! assert_eq!(out[0].normal, [1.0, 0.0]);
//! assert_eq!(out[0].depth, 4.0);
//! ```

pub use mikan_collision_sys::{Aabb, Contact, Pair};

pub mod broadphase;
pub mod reference;

pub use broadphase::Broadphase;

/// Computes one [`Contact`] per pair, in pair order, through the C kernel.
///
/// `out` must have room for at least `pairs.len()` contacts. See
/// [`mikan_collision_sys::contacts`] for the full contract; this is the same
/// function through the crate's public face.
pub fn contacts(bounds: &[Aabb], pairs: &[Pair], out: &mut [Contact]) {
    mikan_collision_sys::contacts(bounds, pairs, out);
}
