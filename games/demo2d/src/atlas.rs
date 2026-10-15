//! The demo's procedural texture atlas.
//!
//! Textures are generated in code so the demo needs no image-loading
//! dependency; pixels are linear RGBA8, which is the format
//! [`Renderer::create_texture`](mikan::Renderer::create_texture)
//! expects. The atlas holds one tile per demo element, and the demo selects a
//! tile with [`tile_uv`] — the same pattern a loaded sprite sheet would use.

use mikan::{Color, UvRect};

/// Edge length of one tile, in pixels.
pub const TILE_SIZE: u32 = 32;

/// Tiles per atlas row.
pub const ATLAS_COLUMNS: u32 = 2;

/// Atlas rows.
pub const ATLAS_ROWS: u32 = 1;

/// Tile drawn for the player square.
pub const PLAYER_TILE: u32 = 0;

/// Tile drawn for the corner marker.
pub const MARKER_TILE: u32 = 1;

/// Width of the border ring painted along the tile edges, in texels.
const BORDER: u32 = 3;

/// Extent of the diagonal highlight in the top-left corner, in texels.
///
/// A diagonal edge cannot be produced by an axis-aligned rectangle, so the
/// capture checks can prove that the square is textured rather than flat.
const DIAGONAL: u32 = 16;

/// The generated atlas: dimensions and linear RGBA8 pixels.
#[derive(Debug)]
pub struct Atlas {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Tightly packed linear RGBA8 pixels, row-major, top row first.
    pub pixels: Vec<u8>,
}

/// Colors one tile is painted with.
#[derive(Debug, Clone, Copy)]
struct Palette {
    /// Fill color of the tile body.
    base: Color,
    /// Color of the border ring.
    border: Color,
    /// Color of the diagonal highlight.
    accent: Color,
}

/// Palette of the player tile.
const PLAYER_PALETTE: Palette = Palette {
    base: Color::rgb(0.98, 0.55, 0.15),
    border: Color::rgb(0.45, 0.22, 0.05),
    accent: Color::rgb(1.0, 0.85, 0.55),
};

/// Palette of the marker tile.
const MARKER_PALETTE: Palette = Palette {
    base: Color::rgb(0.35, 0.75, 0.95),
    border: Color::rgb(0.10, 0.30, 0.50),
    accent: Color::rgb(0.85, 0.95, 1.0),
};

/// Builds the demo atlas: a player tile and a marker tile, side by side.
#[must_use]
pub fn build() -> Atlas {
    let width = TILE_SIZE * ATLAS_COLUMNS;
    let height = TILE_SIZE * ATLAS_ROWS;
    let mut pixels = vec![0; (width * height * 4) as usize];
    paint_tile(&mut pixels, width, PLAYER_TILE, &PLAYER_PALETTE);
    paint_tile(&mut pixels, width, MARKER_TILE, &MARKER_PALETTE);
    Atlas {
        width,
        height,
        pixels,
    }
}

/// The UV rectangle of a tile in the atlas.
///
/// Panics only if `index` is outside the tile grid, which is a bug in this
/// module: `tile_rectangles_are_inside_the_atlas` checks every defined tile.
#[must_use]
pub fn tile_uv(index: u32) -> UvRect {
    UvRect::tile(ATLAS_COLUMNS, ATLAS_ROWS, index)
        .expect("the demo defines exactly as many tiles as the atlas holds")
}

/// Paints one tile: a flat body, a border ring, and a diagonal highlight
/// across the top-left corner. The border wins where the two would overlap.
fn paint_tile(pixels: &mut [u8], atlas_width: u32, index: u32, palette: &Palette) {
    let origin_x = (index % ATLAS_COLUMNS) * TILE_SIZE;
    let origin_y = (index / ATLAS_COLUMNS) * TILE_SIZE;
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let on_border =
                x < BORDER || y < BORDER || x >= TILE_SIZE - BORDER || y >= TILE_SIZE - BORDER;
            let on_diagonal = x + y < DIAGONAL;
            let color = if on_border {
                palette.border
            } else if on_diagonal {
                palette.accent
            } else {
                palette.base
            };
            let offset = (((origin_y + y) * atlas_width + origin_x + x) * 4) as usize;
            pixels[offset] = linear_to_u8(color.r);
            pixels[offset + 1] = linear_to_u8(color.g);
            pixels[offset + 2] = linear_to_u8(color.b);
            pixels[offset + 3] = linear_to_u8(color.a);
        }
    }
}

/// Converts a linear color component to an 8-bit texel value.
fn linear_to_u8(component: f32) -> u8 {
    (component.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the pixel at `(x, y)` of the atlas.
    fn pixel(atlas: &Atlas, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * atlas.width + x) * 4) as usize;
        atlas.pixels[offset..offset + 4].try_into().unwrap()
    }

    #[test]
    fn the_atlas_holds_every_tile() {
        let atlas = build();
        assert_eq!(atlas.width, TILE_SIZE * ATLAS_COLUMNS);
        assert_eq!(atlas.height, TILE_SIZE * ATLAS_ROWS);
        assert_eq!(
            atlas.pixels.len(),
            (atlas.width * atlas.height * 4) as usize,
            "linear RGBA8, tightly packed"
        );
    }

    #[test]
    fn tiles_are_painted_from_their_palettes() {
        let atlas = build();
        let player_base = pixel(&atlas, TILE_SIZE / 2, TILE_SIZE / 2);
        let marker_base = pixel(&atlas, TILE_SIZE + TILE_SIZE / 2, TILE_SIZE / 2);
        assert_ne!(
            player_base, marker_base,
            "tile 1 must not share tile 0's pixels"
        );
        assert_eq!(player_base, [250, 140, 38, 255], "player base is orange");
        assert_eq!(marker_base, [89, 191, 242, 255], "marker base is blue");
    }

    #[test]
    fn the_border_and_diagonal_are_visible() {
        let atlas = build();
        let base = pixel(&atlas, TILE_SIZE / 2, TILE_SIZE / 2);
        let corner = pixel(&atlas, 0, 0);
        let border = pixel(&atlas, TILE_SIZE / 2, 1);
        let diagonal = pixel(&atlas, 4, 4);
        assert_ne!(corner, base, "the top-left corner is darker");
        assert_eq!(corner, border, "corner and edge share the border color");
        assert_ne!(diagonal, base, "the diagonal highlight is lighter");
        assert_ne!(diagonal, border, "the highlight is not the border");
    }

    #[test]
    fn tile_rectangles_are_inside_the_atlas() {
        for (index, expected) in [
            (PLAYER_TILE, UvRect::new([0.0, 0.0], [0.5, 1.0])),
            (MARKER_TILE, UvRect::new([0.5, 0.0], [1.0, 1.0])),
        ] {
            assert_eq!(tile_uv(index), expected, "tile {index}");
        }
    }
}
