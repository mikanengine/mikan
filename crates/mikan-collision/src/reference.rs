//! The Rust reference implementation of the narrow-phase math.
//!
//! This module exists for two concrete reasons, per ADR 0010:
//!
//! - **Correctness oracle.** It is an independent implementation of the exact
//!   contract the C kernel promises, and the test suite cross-checks the two
//!   against each other — fixed cases, boundary cases, and randomized pair
//!   sets. If they ever drift, the tests fail.
//! - **Benchmark baseline.** `examples/bench.rs` runs the same pair sets
//!   through both, so the measured numbers are an honest comparison rather
//!   than an assertion.
//!
//! Production callers use [`crate::contacts`] (the C kernel). This module is
//! deliberately the plain expression of the same rules, statement for
//! statement, so the cross-check is meaningful; it is not dead code.

use crate::{Aabb, Contact, Pair};

/// The comparison-based minimum, mirroring `min_f` in `contacts.c` exactly —
/// including NaN propagation, which is what keeps NaN boxes separated on both
/// paths.
fn min_f(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

/// The comparison-based maximum, mirroring `max_f` in `contacts.c`.
fn max_f(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

/// Mirrors `is_finite_box` in `contacts.c`.
fn is_finite_box(box_: &Aabb) -> bool {
    box_.min[0].is_finite()
        && box_.min[1].is_finite()
        && box_.max[0].is_finite()
        && box_.max[1].is_finite()
}

/// The reference contact for one pair, with the same semantics the C kernel
/// documents in `mikan_contacts.h`.
///
/// ```
/// use mikan_collision::{Aabb, reference};
///
/// let a = Aabb::from_center_size([0.0, 0.0], [10.0, 10.0]);
/// let b = Aabb::from_center_size([0.0, 8.0], [10.0, 10.0]);
/// let contact = reference::contact(&a, &b);
/// assert_eq!(contact.normal, [0.0, 1.0]);
/// assert_eq!(contact.depth, 2.0);
/// ```
#[must_use]
// `!(x > 0.0)` is deliberate and mirrors the C expression exactly: it is
// the form that treats NaN as separated. Rewriting it as `x <= 0.0` would
// change the NaN result and break the cross-check tests.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn contact(a: &Aabb, b: &Aabb) -> Contact {
    if !is_finite_box(a) || !is_finite_box(b) {
        return Contact {
            normal: [0.0, 0.0],
            depth: 0.0,
        };
    }

    let overlap_x = min_f(a.max[0], b.max[0]) - max_f(a.min[0], b.min[0]);
    let overlap_y = min_f(a.max[1], b.max[1]) - max_f(a.min[1], b.min[1]);

    if !(overlap_x > 0.0) || !(overlap_y > 0.0) {
        return Contact {
            normal: [0.0, 0.0],
            depth: 0.0,
        };
    }

    if overlap_x <= overlap_y {
        let normal_x = if a.min[0] + a.max[0] <= b.min[0] + b.max[0] {
            1.0
        } else {
            -1.0
        };
        Contact {
            normal: [normal_x, 0.0],
            depth: overlap_x,
        }
    } else {
        let normal_y = if a.min[1] + a.max[1] <= b.min[1] + b.max[1] {
            1.0
        } else {
            -1.0
        };
        Contact {
            normal: [0.0, normal_y],
            depth: overlap_y,
        }
    }
}

/// The reference batch result for every pair, in pair order.
///
/// `out` must have room for at least `pairs.len()` contacts.
///
/// # Panics
///
/// If `out` is shorter than `pairs`, or if a pair index is out of `bounds`.
pub fn contacts(bounds: &[Aabb], pairs: &[Pair], out: &mut [Contact]) {
    assert!(
        out.len() >= pairs.len(),
        "out needs one contact per pair ({} pairs, {} slots)",
        pairs.len(),
        out.len()
    );
    for (slot, pair) in out.iter_mut().zip(pairs) {
        *slot = contact(&bounds[pair.first as usize], &bounds[pair.second as usize]);
    }
}
