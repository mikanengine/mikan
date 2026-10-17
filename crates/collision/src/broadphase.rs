//! A uniform-grid broad phase.
//!
//! [`Broadphase::rebuild`] sorts boxes into fixed-size cells (a box is
//! inserted into every cell it touches), and [`Broadphase::pairs`] reports
//! every candidate pair that shares at least one cell, deduplicated and
//! sorted. "Candidate" is exactly right: the grid can produce pairs whose
//! boxes do not overlap (they share a cell at a corner, say), and the narrow
//! phase reports those as separated. It can never *miss* an overlapping
//! pair: two boxes that overlap share the cell that contains a point of
//! their intersection.
//!
//! The output is deterministic: `pairs` sorts and deduplicates, so the hash
//! map's iteration order never reaches a caller. That is what lets the demo's
//! collision step stay reproducible (ADR 0009's determinism contract), at the
//! cost of one sort per step at gameplay scale.

use std::collections::HashMap;

use crate::{Aabb, Pair};

/// A uniform-grid broad phase.
///
/// The grid owns no boxes; [`Broadphase::rebuild`] is given the current slice
/// and [`Broadphase::pairs`] reads the grid. Reuse one instance across steps
/// to keep the buckets' allocations.
#[derive(Debug, Clone)]
pub struct Broadphase {
    cell_size: f32,
    buckets: HashMap<(i32, i32), Vec<u32>>,
}

impl Broadphase {
    /// Creates an empty grid whose cells are `cell_size` units wide and tall.
    ///
    /// # Panics
    ///
    /// If `cell_size` is not finite and positive.
    #[must_use]
    pub fn new(cell_size: f32) -> Self {
        assert!(
            cell_size.is_finite() && cell_size > 0.0,
            "cell size must be finite and positive, got {cell_size}"
        );
        Self {
            cell_size,
            buckets: HashMap::new(),
        }
    }

    /// The configured cell size.
    #[must_use]
    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// Replaces the grid's contents with the cells the given boxes touch.
    ///
    /// Degenerate boxes (`min == max` on either axis) and boxes with
    /// non-finite or inverted coordinates are skipped: they can never overlap
    /// anything under the separation definition, so they are not candidates.
    ///
    /// Cells are emptied in place, so a reused grid keeps every cell's
    /// allocation; cells that become empty stay allocated for the next step.
    // `!(min < max)` is deliberate: it also rejects NaN coordinates, which
    // the comparison form does and `min >= max` does not.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn rebuild(&mut self, bounds: &[Aabb]) {
        for bucket in self.buckets.values_mut() {
            bucket.clear();
        }
        for (index, aabb) in bounds.iter().enumerate() {
            if !(aabb.min[0] < aabb.max[0])
                || !(aabb.min[1] < aabb.max[1])
                || !aabb.min[0].is_finite()
                || !aabb.min[1].is_finite()
                || !aabb.max[0].is_finite()
                || !aabb.max[1].is_finite()
            {
                continue;
            }
            let index = index as u32;
            let min_x = (aabb.min[0] / self.cell_size).floor() as i32;
            let min_y = (aabb.min[1] / self.cell_size).floor() as i32;
            let max_x = (aabb.max[0] / self.cell_size).floor() as i32;
            let max_y = (aabb.max[1] / self.cell_size).floor() as i32;
            for x in min_x..=max_x {
                for y in min_y..=max_y {
                    self.buckets.entry((x, y)).or_default().push(index);
                }
            }
        }
    }

    /// Writes every deduplicated candidate pair into `out`, sorted by
    /// `(first, second)`, after clearing it.
    pub fn pairs(&self, out: &mut Vec<Pair>) {
        out.clear();
        for bucket in self.buckets.values() {
            for (position, &first) in bucket.iter().enumerate() {
                for &second in &bucket[position + 1..] {
                    let (first, second) = if first <= second {
                        (first, second)
                    } else {
                        (second, first)
                    };
                    out.push(Pair { first, second });
                }
            }
        }
        out.sort_unstable_by_key(|pair| (pair.first, pair.second));
        out.dedup();
    }
}
