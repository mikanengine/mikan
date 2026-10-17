//! Texture-coordinate rectangles and the math that maps pixel regions to them.
//!
//! [`UvRect`] lives in `mikan-core` because both the render layer (sprite
//! sampling) and the asset layer (sprite-sheet frames) produce and consume it,
//! and it has no graphics-API dependency.

/// A rectangle in normalized texture coordinates.
///
/// `[0.0, 0.0]` is the top-left corner of a texture and `[1.0, 1.0]` the
/// bottom-right, matching the y-down world. A sub-rectangle selects one region
/// of a texture atlas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvRect {
    /// Top-left corner of the region.
    pub min: [f32; 2],
    /// Bottom-right corner of the region.
    pub max: [f32; 2],
}

impl UvRect {
    /// The whole texture.
    pub const FULL: Self = Self {
        min: [0.0, 0.0],
        max: [1.0, 1.0],
    };

    /// Creates a region from two corners.
    #[must_use]
    pub const fn new(min: [f32; 2], max: [f32; 2]) -> Self {
        Self { min, max }
    }

    /// The region of tile `index` in a `columns` by `rows` atlas grid.
    ///
    /// Tiles are numbered row-major from the top-left. Returns `None` when the
    /// grid is empty or `index` is outside it, so callers can report the
    /// mistake instead of sampling the wrong region.
    #[must_use]
    pub fn tile(columns: u32, rows: u32, index: u32) -> Option<Self> {
        if columns == 0 || rows == 0 || u64::from(index) >= u64::from(columns) * u64::from(rows) {
            return None;
        }
        let width = 1.0 / columns as f32;
        let height = 1.0 / rows as f32;
        let column = (index % columns) as f32;
        let row = (index / columns) as f32;
        let min = [column * width, row * height];
        Some(Self {
            min,
            max: [min[0] + width, min[1] + height],
        })
    }

    /// Shrinks the region by `amount` on every side.
    ///
    /// The usual reason is atlas bleeding: a linear sampler that reaches the
    /// exact edge of a region interpolates with the neighbouring texels, so
    /// insetting by half a texel (`0.5 / texture_size`) makes every sampled
    /// location fall inside the region being selected.
    ///
    /// A one-texel-wide region collapses to the texel's center point — the
    /// correct sample location for it — and stays collapsed. A region too
    /// small to survive the inset otherwise (the sides would cross) is
    /// returned unchanged, so the result never becomes inverted.
    #[must_use]
    pub fn inset(self, amount: [f32; 2]) -> Self {
        let min = [self.min[0] + amount[0], self.min[1] + amount[1]];
        let max = [self.max[0] - amount[0], self.max[1] - amount[1]];
        if min[0] <= max[0] && min[1] <= max[1] {
            Self { min, max }
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_grid_is_row_major_from_the_top_left() {
        assert_eq!(UvRect::tile(1, 1, 0), Some(UvRect::FULL));
        assert_eq!(
            UvRect::tile(2, 1, 1),
            Some(UvRect::new([0.5, 0.0], [1.0, 1.0]))
        );
        assert_eq!(
            UvRect::tile(2, 2, 2),
            Some(UvRect::new([0.0, 0.5], [0.5, 1.0])),
            "index 2 starts the second row"
        );
        assert_eq!(
            UvRect::tile(2, 2, 3),
            Some(UvRect::new([0.5, 0.5], [1.0, 1.0]))
        );
    }

    #[test]
    fn tile_rejects_empty_grids_and_out_of_range_tiles() {
        assert_eq!(UvRect::tile(0, 4, 0), None);
        assert_eq!(UvRect::tile(4, 0, 0), None);
        assert_eq!(UvRect::tile(2, 2, 4), None);
        assert_eq!(UvRect::tile(2, 2, u32::MAX), None);
    }

    #[test]
    fn inset_shrinks_on_every_side() {
        let uv = UvRect::new([0.25, 0.5], [0.75, 1.0]);
        let inset = uv.inset([0.125, 0.1]);
        assert_eq!(inset, UvRect::new([0.375, 0.6], [0.625, 0.9]));
    }

    #[test]
    fn inset_collapses_a_one_texel_region_to_its_center() {
        // A one-texel-high strip of an 8-texel texture: inset by half a texel
        // lands exactly on the texel's center line, which is the right place
        // to sample it — not the original rect, and not an inverted one.
        let strip = UvRect::new([0.0, 0.0], [1.0, 0.125]);
        assert_eq!(
            strip.inset([0.0, 0.0625]),
            UvRect::new([0.0, 0.0625], [1.0, 0.0625])
        );
    }

    #[test]
    fn inset_never_inverts_a_region_already_too_small() {
        let empty = UvRect::new([0.5, 0.5], [0.5, 0.5]);
        assert_eq!(empty.inset([0.1, 0.1]), empty, "stays a point");
        let thin = UvRect::new([0.0, 0.0], [0.4, 0.4]);
        assert_eq!(
            thin.inset([0.25, 0.25]),
            thin,
            "a region that cannot lose the inset on all sides is unchanged"
        );
    }
}
