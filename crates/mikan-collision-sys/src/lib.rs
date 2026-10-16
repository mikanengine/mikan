//! Raw interface to the C narrow-phase kernel (`mikan_contacts`).
//!
//! This crate is the ABI mirror of `src/mikan_contacts.h`: the `#[repr(C)]`
//! data types, the `extern "C"` declaration, and one safe batch entry point.
//! `mikan-collision` is the crate applications should depend on; it
//! re-exports the data types and adds the broad phase, the reference
//! implementation, and the guides. Nothing here is engine-specific.
//!
//! # Why `unsafe` is allowed in this crate
//!
//! The workspace forbids `unsafe_code` crate-wide. This is the one crate in
//! the repository where an `unsafe` block is legitimate: it is the exact
//! boundary between Rust and the C kernel, and the rules that make the call
//! sound — pointer validity, lengths, non-aliasing — are a C ABI contract the
//! Rust type system cannot check. The override is recorded here, per
//! `docs/ffi.md`; every occurrence carries a `// SAFETY:` comment; the safe
//! wrapper (`mikan-collision`) and every consumer contain none.
//!
//! # Layout
//!
//! All three types are plain data with a defined layout, read and written by
//! value across the boundary; no pointers are ever retained by C. `Pair`
//! uses `u32` for the C `unsigned int`, which is 32-bit on every target
//! Mikan supports.

/// An axis-aligned bounding box in world space.
///
/// Layout-identical to `mikan_aabb` in `src/mikan_contacts.h`. An empty box
/// (`min == max`) is valid and never overlaps anything, not even itself.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    /// The corner with the smallest coordinates.
    pub min: [f32; 2],
    /// The corner with the largest coordinates.
    pub max: [f32; 2],
}

impl Aabb {
    /// Builds a box from its center and full size.
    ///
    /// A zero or negative size component produces a degenerate box, which the
    /// contact math treats as separated from everything.
    #[must_use]
    pub const fn from_center_size(center: [f32; 2], size: [f32; 2]) -> Self {
        let half_x = size[0] / 2.0;
        let half_y = size[1] / 2.0;
        Self {
            min: [center[0] - half_x, center[1] - half_y],
            max: [center[0] + half_x, center[1] + half_y],
        }
    }

    /// The box center.
    #[must_use]
    pub const fn center(&self) -> [f32; 2] {
        [
            (self.min[0] + self.max[0]) / 2.0,
            (self.min[1] + self.max[1]) / 2.0,
        ]
    }

    /// The box width and height.
    #[must_use]
    pub const fn size(&self) -> [f32; 2] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1]]
    }
}

/// An index pair into a slice of [`Aabb`]s: the candidate two boxes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pair {
    /// The index of the first box (the contact normal points away from it).
    pub first: u32,
    /// The index of the second box (the contact normal points toward it).
    pub second: u32,
}

/// The narrow-phase result for one [`Pair`].
///
/// `depth > 0` means the boxes overlap, and `normal` is a unit axis vector
/// pointing from the pair's `first` box toward its `second` box. A `depth` of
/// zero means the pair is separated (including boxes that merely touch).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// The unit axis vector that separates `second` from `first`, or zero.
    pub normal: [f32; 2],
    /// The overlap along `normal` when positive; zero when separated.
    pub depth: f32,
}

impl Contact {
    /// Whether the pair overlaps (`depth > 0`).
    #[must_use]
    pub fn is_overlap(&self) -> bool {
        self.depth > 0.0
    }
}

unsafe extern "C" {
    /// The raw C binding, exposed for `-sys`-style completeness and for tests
    /// of the kernel's defensive validation. Prefer [`contacts`], which is
    /// safe and adds debug assertions.
    ///
    /// # Safety
    ///
    /// The caller must ensure that:
    ///
    /// - `bounds` points to `bounds_len` readable [`Aabb`]s (or `bounds_len`
    ///   is zero and the pointer is suitably aligned);
    /// - `pairs` points to `pair_count` readable [`Pair`]s;
    /// - `out` points to `pair_count` writable [`Contact`]s;
    /// - `out` does not alias `bounds` or `pairs`.
    ///
    /// The kernel itself treats null pointers and out-of-range indices as
    /// separated results rather than undefined behavior, but Rust callers
    /// should not rely on that instead of upholding the contract.
    pub fn mikan_contacts(
        bounds: *const Aabb,
        bounds_len: usize,
        pairs: *const Pair,
        pair_count: usize,
        out: *mut Contact,
    );
}

/// Computes one [`Contact`] per pair, in pair order.
///
/// `out` must have room for at least `pairs.len()` contacts; the first
/// `pairs.len()` entries are written. Indices out of `bounds` are treated as
/// separated by the kernel itself, so no unsafe input can occur even if a
/// caller bypasses the debug assertions.
///
/// # Panics
///
/// In debug builds only: if `out` is shorter than `pairs`, or if a pair index
/// is out of `bounds`. Release builds cannot panic.
pub fn contacts(bounds: &[Aabb], pairs: &[Pair], out: &mut [Contact]) {
    debug_assert!(
        out.len() >= pairs.len(),
        "out needs one contact per pair ({} pairs, {} slots)",
        pairs.len(),
        out.len()
    );
    debug_assert!(
        pairs
            .iter()
            .all(|p| (p.first as usize) < bounds.len() && (p.second as usize) < bounds.len()),
        "pair indices must be within bounds"
    );

    // SAFETY: `bounds`, `pairs`, and `out` are live slices (each with a valid
    // pointer and its own length) and, because `out` is borrowed mutably
    // while the other two are borrowed immutably, they cannot alias. The
    // kernel reads at most `bounds.len()` boxes and `pairs.len()` pairs, and
    // writes exactly `pairs.len()` contacts into `out`; it retains no
    // pointer and touches no other memory.
    unsafe {
        mikan_contacts(
            bounds.as_ptr(),
            bounds.len(),
            pairs.as_ptr(),
            pairs.len(),
            out.as_mut_ptr(),
        );
    }
}
