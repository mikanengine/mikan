//! A minimal 2D camera: what part of world space is visible.
//!
//! The camera is deliberately small. It has no rotation, no bounds, and no
//! smoothing; a camera that follows a player is a few lines of game code, and
//! one that needs more behavior can be built from this type. What it does have
//! is the pair of transformations every 2D game needs — world to screen and
//! screen to world — and an optional pixel-perfect mode that snaps the camera
//! to a whole-pixel grid for pixel art.
//!
//! World space uses the same axes as the window: `x` grows to the right and
//! `y` grows downward. That keeps cursor coordinates usable as world
//! coordinates without a flip. The conversion into clip space is a rendering
//! convention and lives in `mikan-render`.

/// Describes the visible region of world space.
///
/// [`position`](Self::position) is the world point shown at the center of the
/// viewport, and [`zoom`](Self::zoom) is how many physical pixels one world
/// unit covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera2d {
    /// World-space point that appears at the center of the viewport.
    pub position: [f32; 2],
    /// Physical pixels per world unit. Values that are not finite and greater
    /// than zero are treated as `1.0` when rendering (see
    /// [`effective_zoom`](Self::effective_zoom)).
    pub zoom: f32,
}

impl Camera2d {
    /// Creates a camera centered on `position` with the given `zoom`.
    #[must_use]
    pub const fn new(position: [f32; 2], zoom: f32) -> Self {
        Self { position, zoom }
    }

    /// The camera that maps world units 1:1 to physical pixels, with the world
    /// origin at the top-left corner of `viewport`.
    ///
    /// This is the rendering behavior of a frame without a camera, expressed as
    /// a camera so both paths share one implementation.
    #[must_use]
    pub fn pixel_space(viewport: [f32; 2]) -> Self {
        Self {
            position: [viewport[0] * 0.5, viewport[1] * 0.5],
            zoom: 1.0,
        }
    }

    /// The zoom used for rendering.
    ///
    /// A zoom that is not a finite number greater than zero would produce a
    /// degenerate or NaN transform, so it falls back to `1.0` instead of
    /// corrupting the frame.
    #[must_use]
    pub fn effective_zoom(self) -> f32 {
        if self.zoom.is_finite() && self.zoom > 0.0 {
            self.zoom
        } else {
            1.0
        }
    }

    /// Maps a world-space point to its position on screen, in physical pixels
    /// with the origin at the top-left corner of the viewport.
    ///
    /// This is the reference transformation the render layer's matrix must
    /// agree with; it is also what gameplay needs to turn a cursor position
    /// into a world position once picking exists.
    #[must_use]
    pub fn world_to_screen(self, viewport: [f32; 2], world: [f32; 2]) -> [f32; 2] {
        let zoom = self.effective_zoom();
        [
            (world[0] - self.position[0]) * zoom + viewport[0] * 0.5,
            (world[1] - self.position[1]) * zoom + viewport[1] * 0.5,
        ]
    }

    /// Maps a point on screen (physical pixels, origin at the top-left corner
    /// of the viewport) back to world space.
    ///
    /// The exact inverse of [`world_to_screen`](Self::world_to_screen) for
    /// every zoom, including zooms that render as `1.0`.
    #[must_use]
    pub fn screen_to_world(self, viewport: [f32; 2], screen: [f32; 2]) -> [f32; 2] {
        let zoom = self.effective_zoom();
        [
            (screen[0] - viewport[0] * 0.5) / zoom + self.position[0],
            (screen[1] - viewport[1] * 0.5) / zoom + self.position[1],
        ]
    }

    /// Returns the same camera with its position moved to a whole physical
    /// pixel: the world origin maps to an integer screen coordinate and,
    /// because the offset is a whole number, every other world point keeps its
    /// fractional position relative to the origin.
    ///
    /// With an integer zoom — the zoom [`pixel_perfect`](Self::pixel_perfect)
    /// selects — this means whole world coordinates land on whole physical
    /// pixels. The shift is at most half a rendered world unit, so a camera
    /// that follows something still tracks it, just never sub-pixel.
    #[must_use]
    pub fn snap_to_physical_pixels(self, viewport: [u32; 2]) -> Self {
        let zoom = self.effective_zoom();
        let half = [viewport[0] as f32 * 0.5, viewport[1] as f32 * 0.5];
        let snap = |component: f32, half: f32| ((component * zoom - half).round() + half) / zoom;
        Self {
            position: [
                snap(self.position[0], half[0]),
                snap(self.position[1], half[1]),
            ],
            zoom: self.zoom,
        }
    }

    /// A pixel-perfect camera: an integer zoom plus a position on the physical
    /// pixel grid, suitable for pixel art.
    ///
    /// The zoom is the largest integer scale at which `virtual_size` (a
    /// virtual-resolution screen, in virtual pixels) fits inside `viewport`
    /// (see [`pixel_perfect_scale`]). One virtual pixel — one world unit —
    /// therefore covers a whole number of physical pixels, and
    /// [`snap_to_physical_pixels`](Self::snap_to_physical_pixels) keeps the
    /// grid aligned to the window instead of drifting half a pixel when the
    /// window size is odd.
    ///
    /// The documented limits: the virtual screen is centred to within one
    /// physical pixel (there is no letterbox fill, the clear color shows
    /// through); a viewport smaller than `virtual_size` renders at scale 1
    /// with the same centered behavior; and only the camera is snapped — a
    /// game that wants its sprites on the virtual grid must place them on it
    /// (for example by rounding positions to `1 / scale`).
    #[must_use]
    pub fn pixel_perfect(position: [f32; 2], viewport: [u32; 2], virtual_size: [u32; 2]) -> Self {
        let zoom = pixel_perfect_scale(viewport, virtual_size) as f32;
        Self::new(position, zoom).snap_to_physical_pixels(viewport)
    }
}

/// The largest integer scale at which a `virtual_size` pixel grid fits inside
/// `viewport`, at least `1`.
///
/// This is the zoom a pixel-perfect view uses: one virtual pixel covers
/// exactly `scale` physical pixels, never a fractional number of them. A
/// degenerate virtual size (a zero dimension) or a viewport smaller than the
/// virtual grid both fall back to `1`, so the result is always a usable zoom.
#[must_use]
pub fn pixel_perfect_scale(viewport: [u32; 2], virtual_size: [u32; 2]) -> u32 {
    if virtual_size[0] == 0 || virtual_size[1] == 0 {
        return 1;
    }
    (viewport[0] / virtual_size[0])
        .min(viewport[1] / virtual_size[1])
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: [f32; 2] = [960.0, 600.0];

    fn close(actual: [f32; 2], expected: [f32; 2]) {
        for axis in 0..2 {
            assert!(
                (actual[axis] - expected[axis]).abs() < 1e-4,
                "axis {axis}: expected {expected:?}, got {actual:?}"
            );
        }
    }

    #[test]
    fn pixel_space_camera_is_identity_from_the_top_left() {
        let camera = Camera2d::pixel_space(VIEWPORT);
        close(camera.world_to_screen(VIEWPORT, [0.0, 0.0]), [0.0, 0.0]);
        close(camera.world_to_screen(VIEWPORT, VIEWPORT), VIEWPORT);
        close(
            camera.world_to_screen(VIEWPORT, [480.0, 300.0]),
            [480.0, 300.0],
        );
    }

    #[test]
    fn the_camera_position_maps_to_the_viewport_center() {
        let camera = Camera2d::new([123.0, -45.0], 2.0);
        close(
            camera.world_to_screen(VIEWPORT, camera.position),
            [480.0, 300.0],
        );
    }

    #[test]
    fn zoom_scales_offsets_around_the_center() {
        let camera = Camera2d::new([100.0, 100.0], 2.0);
        // One world unit is two pixels, so a point 10 world units right of the
        // camera lands 20 pixels right of the center.
        close(
            camera.world_to_screen(VIEWPORT, [110.0, 100.0]),
            [500.0, 300.0],
        );
        close(
            camera.world_to_screen(VIEWPORT, [100.0, 90.0]),
            [480.0, 280.0],
        );
    }

    #[test]
    fn fractional_zoom_scales_down() {
        let camera = Camera2d::new([0.0, 0.0], 0.5);
        close(
            camera.world_to_screen(VIEWPORT, [200.0, 200.0]),
            [580.0, 400.0],
        );
    }

    #[test]
    fn invalid_zoom_falls_back_to_one_to_one() {
        for zoom in [0.0, -3.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let camera = Camera2d::new([10.0, 10.0], zoom);
            assert_eq!(camera.effective_zoom(), 1.0, "zoom {zoom}");
            close(
                camera.world_to_screen(VIEWPORT, [10.0, 10.0]),
                [480.0, 300.0],
            );
        }
        assert_eq!(Camera2d::new([0.0, 0.0], 2.5).effective_zoom(), 2.5);
    }

    #[test]
    fn screen_to_world_inverts_world_to_screen() {
        let worlds = [[0.0, 0.0], [100.0, 100.0], [-250.0, 480.0], VIEWPORT];
        for camera in [
            Camera2d::new([0.0, 0.0], 1.0),
            Camera2d::new([120.0, -40.0], 2.5),
            Camera2d::new([-500.0, 900.0], 0.35),
            Camera2d::new([10.0, 10.0], 0.0),
        ] {
            for world in worlds {
                let screen = camera.world_to_screen(VIEWPORT, world);
                close(camera.screen_to_world(VIEWPORT, screen), world);
            }
        }
    }

    #[test]
    fn pixel_perfect_scale_is_the_largest_fitting_integer() {
        assert_eq!(pixel_perfect_scale([960, 600], [240, 150]), 4);
        assert_eq!(pixel_perfect_scale([960, 600], [320, 200]), 3);
        assert_eq!(
            pixel_perfect_scale([960, 600], [320, 240]),
            2,
            "limited by the tighter axis (600 / 240 = 2)"
        );
        assert_eq!(pixel_perfect_scale([961, 601], [240, 150]), 4);
        assert_eq!(
            pixel_perfect_scale([100, 100], [240, 150]),
            1,
            "a viewport smaller than the virtual grid still renders at 1:1"
        );
        assert_eq!(
            pixel_perfect_scale([960, 600], [0, 150]),
            1,
            "a degenerate virtual size falls back to 1"
        );
        assert_eq!(pixel_perfect_scale([960, 600], [960, 600]), 1);
    }

    #[test]
    fn pixel_perfect_camera_puts_whole_world_coordinates_on_whole_pixels() {
        for viewport in [[960u32, 600u32], [961, 601], [959, 599], [1920, 1080]] {
            for virtual_size in [[240u32, 150u32], [320, 200], [480, 300], [64, 64]] {
                let camera = Camera2d::pixel_perfect([120.4, 75.9], viewport, virtual_size);
                let expected_zoom = pixel_perfect_scale(viewport, virtual_size) as f32;
                assert_eq!(camera.effective_zoom(), expected_zoom);
                let viewport_f = [viewport[0] as f32, viewport[1] as f32];
                for world in [[0.0, 0.0], [1.0, 1.0], [10.0, 37.0], [-5.0, 4.0]] {
                    let screen = camera.world_to_screen(viewport_f, world);
                    for axis in 0..2 {
                        let fraction = screen[axis] - screen[axis].round();
                        assert!(
                            fraction.abs() < 1e-3,
                            "viewport {viewport:?} virtual {virtual_size:?}: world {world:?} \
                             landed on {screen:?}, not a whole pixel"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn pixel_perfect_snapping_moves_the_camera_by_at_most_half_a_virtual_pixel() {
        let requested = [120.4, 75.9];
        let camera = Camera2d::pixel_perfect(requested, [961, 601], [240, 150]);
        for (axis, (snapped, requested)) in camera.position.iter().zip(requested).enumerate() {
            let shift = (snapped - requested).abs();
            assert!(
                shift <= 0.5 / camera.effective_zoom() + 1e-4,
                "axis {axis} moved by {shift}"
            );
        }
    }

    #[test]
    fn a_camera_already_on_the_grid_is_unchanged_by_snapping() {
        // With zoom 4, 1/4 world units are whole physical pixels; the snapped
        // position of a request already on that grid is the request itself.
        let requested = [120.25, 75.25];
        let camera = Camera2d::pixel_perfect(requested, [960, 600], [240, 150]);
        close(camera.position, requested);
    }
}
