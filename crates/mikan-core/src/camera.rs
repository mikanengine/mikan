//! A minimal 2D camera: what part of world space is visible.
//!
//! The camera is deliberately small. It has no rotation, no bounds, and no
//! smoothing; a camera that follows a player is a few lines of game code, and
//! one that needs more behavior can be built from this type.
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
}
