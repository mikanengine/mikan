//! The demo's scenes: what each one draws, at documented positions.
//!
//! Three scenes share one loaded [`Assets`]:
//!
//! - [`classic`]: the original demo — a driveable player square and a static
//!   corner marker — now drawn from the sheet's `player` and `marker` frames.
//! - [`showcase`]: a fixed gallery that makes each Phase 3 feature visible at a
//!   known pixel position: rotation, tint, alpha, nearest vs linear sampling,
//!   and PNG alpha from `star.png`.
//! - [`StressLayout`] + [`stress_sprites`]: `N` moving sprites sharing one
//!   texture, for measuring batching and frame cost.
//!
//! The classic and showcase layouts live in world pixels and are deterministic,
//! which is what lets the capture checks assert exact screen positions.

use core::f32::consts::FRAC_PI_4;

use mikan::{Color, SamplerMode, Sprite};

use crate::assets::Assets;

/// Side length of the player square in pixels (classic scene).
pub const PLAYER_SIZE: f32 = 96.0;
/// Player speed in pixels per second (classic scene).
pub const PLAYER_SPEED: f32 = 420.0;
/// Edge length of the static marker (classic scene).
pub const MARKER_SIZE: f32 = 48.0;
/// Distance of the marker from the bottom-left corner (classic scene).
pub const MARKER_MARGIN: f32 = 24.0;

/// The classic scene: the player square and the corner marker.
#[must_use]
pub fn classic<'a>(
    assets: &'a Assets,
    player_center: [f32; 2],
    player_size: f32,
    world_view: [f32; 2],
    sampler: SamplerMode,
) -> [Sprite<'a>; 2] {
    [
        Sprite::new(player_center, [player_size, player_size], &assets.atlas)
            .with_uv(assets.player_uv)
            .with_sampler(sampler),
        Sprite::new(
            marker_center(world_view),
            [MARKER_SIZE, MARKER_SIZE],
            &assets.atlas,
        )
        .with_uv(assets.marker_uv)
        .with_sampler(sampler),
    ]
}

/// Center of the classic scene's corner marker in a `world_view`-sized world.
#[must_use]
pub fn marker_center(world_view: [f32; 2]) -> [f32; 2] {
    let half = MARKER_SIZE * 0.5;
    [
        MARKER_MARGIN + half,
        (world_view[1] - MARKER_MARGIN - half).max(half),
    ]
}

/// Y of the showcase's first row: plain, rotated, tinted, and alpha sprites.
pub const SHOWCASE_ROW1_Y: f32 = 140.0;
/// X of the unmodified player tile in the showcase.
pub const SHOWCASE_X_PLAIN: f32 = 140.0;
/// X of the 45-degree rotated player tile.
pub const SHOWCASE_X_ROTATED: f32 = 340.0;
/// X of the tinted player tile.
pub const SHOWCASE_X_TINTED: f32 = 540.0;
/// X of the half-transparent player tile.
pub const SHOWCASE_X_ALPHA: f32 = 740.0;
/// Edge length of the showcase's row-1 tiles (3x the 32-pixel frame).
pub const SHOWCASE_TILE: f32 = 96.0;
/// Rotation of the showcase's rotated tile: 45 degrees clockwise.
pub const SHOWCASE_ROTATION: f32 = FRAC_PI_4;
/// Tint of the showcase's tinted tile.
pub const SHOWCASE_TINT: Color = Color::rgb(0.25, 0.6, 1.0);
/// Tint of the showcase's half-transparent tile.
pub const SHOWCASE_ALPHA_TINT: Color = Color::new(1.0, 1.0, 1.0, 0.4);

/// Y of the showcase's sampler-comparison row.
pub const SHOWCASE_ROW2_Y: f32 = 330.0;
/// X of the smoothly sampled checker tile.
pub const SHOWCASE_X_LINEAR: f32 = 340.0;
/// X of the crisply sampled checker tile.
pub const SHOWCASE_X_NEAREST: f32 = 620.0;
/// Edge length of the checker pair: 5.5x the 32-pixel tile, so texel edges
/// cannot line up with destination pixels and the two sampler modes differ.
pub const SHOWCASE_CHECKER_SIZE: f32 = 176.0;

/// Center of the showcase's star and its backdrop.
pub const SHOWCASE_STAR_CENTER: [f32; 2] = [480.0, 500.0];
/// Edge length of the star: 4x the 24-pixel texture.
pub const SHOWCASE_STAR_SIZE: f32 = 96.0;
/// Opaque color drawn behind the star, visible through its transparent
/// corners; deliberately unlike both the star and the clear color.
pub const SHOWCASE_BACKDROP: Color = Color::rgb(0.08, 0.12, 0.3);

/// The showcase scene: eight sprites at fixed positions, in painter order.
///
/// The order matters twice over: the backdrop is drawn before the star, and
/// the two checker tiles pin their samplers so the comparison stays honest no
/// matter what `--sampler` says.
#[must_use]
pub fn showcase(assets: &Assets, sampler: SamplerMode) -> Vec<Sprite<'_>> {
    let tile = SHOWCASE_TILE;
    vec![
        Sprite::new(
            [SHOWCASE_X_PLAIN, SHOWCASE_ROW1_Y],
            [tile, tile],
            &assets.atlas,
        )
        .with_uv(assets.player_uv)
        .with_sampler(sampler),
        Sprite::new(
            [SHOWCASE_X_ROTATED, SHOWCASE_ROW1_Y],
            [tile, tile],
            &assets.atlas,
        )
        .with_uv(assets.player_uv)
        .with_rotation(SHOWCASE_ROTATION)
        .with_sampler(sampler),
        Sprite::new(
            [SHOWCASE_X_TINTED, SHOWCASE_ROW1_Y],
            [tile, tile],
            &assets.atlas,
        )
        .with_uv(assets.player_uv)
        .with_color(SHOWCASE_TINT)
        .with_sampler(sampler),
        Sprite::new(
            [SHOWCASE_X_ALPHA, SHOWCASE_ROW1_Y],
            [tile, tile],
            &assets.atlas,
        )
        .with_uv(assets.player_uv)
        .with_color(SHOWCASE_ALPHA_TINT)
        .with_sampler(sampler),
        Sprite::new(
            [SHOWCASE_X_LINEAR, SHOWCASE_ROW2_Y],
            [SHOWCASE_CHECKER_SIZE; 2],
            &assets.atlas,
        )
        .with_uv(assets.checker_uv)
        .with_sampler(SamplerMode::Linear),
        Sprite::new(
            [SHOWCASE_X_NEAREST, SHOWCASE_ROW2_Y],
            [SHOWCASE_CHECKER_SIZE; 2],
            &assets.atlas,
        )
        .with_uv(assets.checker_uv)
        .with_sampler(SamplerMode::Nearest),
        Sprite::solid(
            SHOWCASE_STAR_CENTER,
            [SHOWCASE_STAR_SIZE; 2],
            SHOWCASE_BACKDROP,
        ),
        Sprite::new(SHOWCASE_STAR_CENTER, [SHOWCASE_STAR_SIZE; 2], &assets.star)
            .with_sampler(sampler),
    ]
}

/// Edge length of one stress sprite: half the 32-pixel frame.
pub const STRESS_SPRITE_SIZE: f32 = 16.0;

/// A grid of `count` cell centers covering `viewport`, with room around each
/// center for a sprite to move within its cell without leaving it.
#[derive(Debug, Clone, PartialEq)]
pub struct StressLayout {
    bases: Vec<[f32; 2]>,
    cell: [f32; 2],
}

impl StressLayout {
    /// Lays out `count` sprites over `viewport`, in row-major order.
    #[must_use]
    pub fn new(count: u32, viewport: [f32; 2]) -> Self {
        let viewport = [viewport[0].max(1.0), viewport[1].max(1.0)];
        let aspect = viewport[0] / viewport[1];
        let count_f = count as f32;
        let columns = ((count_f * aspect).sqrt().ceil() as u32).max(1);
        let rows = count.div_ceil(columns).max(1);
        let cell = [viewport[0] / columns as f32, viewport[1] / rows as f32];
        let bases = (0..count)
            .map(|index| {
                let column = index % columns;
                let row = index / columns;
                [
                    (column as f32 + 0.5) * cell[0],
                    (row as f32 + 0.5) * cell[1],
                ]
            })
            .collect();
        Self { bases, cell }
    }

    /// The sprite count the layout was built for.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bases.len()
    }

    /// Cell size in pixels.
    #[must_use]
    pub fn cell(&self) -> [f32; 2] {
        self.cell
    }

    /// The center of sprite `index` at `elapsed` seconds.
    ///
    /// A deterministic Lissajous wobble around the cell center: each sprite
    /// moves within its own cell, and the same `elapsed` always produces the
    /// same frame, which keeps captures and benchmarks comparable.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of range, which is a bug in the caller's loop.
    #[must_use]
    pub fn position(&self, index: usize, elapsed: f32) -> [f32; 2] {
        let base = self.bases[index];
        let phase = index as f32;
        [
            base[0] + (elapsed * 0.7 + phase * 0.37).sin() * self.cell[0] * 0.45,
            base[1] + (elapsed * 1.1 + phase * 0.71).cos() * self.cell[1] * 0.45,
        ]
    }
}

/// Appends the stress scene's sprites to `out`, in index order.
///
/// All of them share one texture and one sampler, so they form a single batch
/// and one draw call however many there are.
pub fn stress_sprites<'a>(
    assets: &'a Assets,
    layout: &StressLayout,
    elapsed: f32,
    sampler: SamplerMode,
    out: &mut Vec<Sprite<'a>>,
) {
    out.reserve(layout.len());
    for index in 0..layout.len() {
        out.push(
            Sprite::new(
                layout.position(index, elapsed),
                [STRESS_SPRITE_SIZE; 2],
                &assets.atlas,
            )
            .with_uv(assets.player_uv)
            .with_sampler(sampler),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_sits_in_the_bottom_left_corner() {
        assert_eq!(marker_center([960.0, 600.0]), [48.0, 552.0]);
        assert_eq!(marker_center([160.0, 100.0]), [48.0, 52.0]);
        assert_eq!(
            marker_center([40.0, 40.0]),
            [48.0, 24.0],
            "a world smaller than the margin clamps the marker instead of \
             pushing it off the top"
        );
    }

    #[test]
    fn the_stress_layout_places_every_sprite_center_inside_the_viewport() {
        let layout = StressLayout::new(10_000, [960.0, 600.0]);
        assert_eq!(layout.len(), 10_000);
        // 127 columns x 79 rows covers 10000 while keeping the aspect ratio.
        let cells = [960.0 / 127.0, 600.0 / 79.0];
        for (axis, expected) in cells.iter().enumerate() {
            assert!(
                (layout.cell()[axis] - expected).abs() < 1e-3,
                "cell {axis}: {:?}",
                layout.cell()
            );
        }
        for index in 0..layout.len() {
            for elapsed in [0.0, 0.25, 1.0, 7.5] {
                let position = layout.position(index, elapsed);
                assert!(
                    (0.0..=960.0).contains(&position[0]) && (0.0..=600.0).contains(&position[1]),
                    "sprite {index} at t={elapsed} left the viewport: {position:?}"
                );
            }
        }
    }

    #[test]
    fn stress_positions_are_deterministic_and_time_dependent() {
        let layout = StressLayout::new(64, [960.0, 600.0]);
        assert_eq!(layout.position(7, 1.25), layout.position(7, 1.25));
        assert_ne!(layout.position(7, 0.0), layout.position(7, 1.25));
        assert_ne!(
            layout.position(7, 1.25),
            layout.position(8, 1.25),
            "neighbours move out of phase"
        );
    }

    #[test]
    fn stress_sprites_stay_within_their_cells() {
        let layout = StressLayout::new(100, [640.0, 480.0]);
        let cell = layout.cell();
        let columns = (640.0 / cell[0]).round() as usize;
        for index in [0usize, 1, 11, 12, 50, 99] {
            let column = (index % columns) as f32;
            let row = (index / columns) as f32;
            let base = [(column + 0.5) * cell[0], (row + 0.5) * cell[1]];
            for elapsed in [0.0, 0.5, 1.0, 2.5, 10.0, 123.456] {
                let position = layout.position(index, elapsed);
                for axis in 0..2 {
                    assert!(
                        (position[axis] - base[axis]).abs() <= cell[axis] * 0.45 + 1e-3,
                        "index {index} at t={elapsed} left its cell: {position:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_degenerate_layout_is_still_usable() {
        let layout = StressLayout::new(1, [0.0, 0.0]);
        assert_eq!(layout.len(), 1);
        assert!(layout.cell()[0] >= 1.0 && layout.cell()[1] >= 1.0);
        assert_eq!(layout.cell(), [1.0, 1.0]);
    }
}
