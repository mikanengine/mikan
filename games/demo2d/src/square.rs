//! The single simulation object of the demo.
//!
//! Movement is integrated in fixed steps and rendered with interpolation, which
//! is the timing model the platform runner provides. Nothing here touches the
//! window, the GPU, or the event loop, so it is all unit-testable.

use core::time::Duration;

/// A square that moves in response to input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Square {
    position: [f32; 2],
    previous: [f32; 2],
    size: f32,
    speed: f32,
}

impl Square {
    /// Creates a square centered on `position`, with a side length and maximum
    /// speed in pixels and pixels per second.
    #[must_use]
    pub fn new(position: [f32; 2], size: f32, speed: f32) -> Self {
        Self {
            position,
            previous: position,
            size,
            speed,
        }
    }

    /// Side length in pixels.
    #[must_use]
    pub fn size(&self) -> f32 {
        self.size
    }

    /// Simulation position, updated only by [`Square::fixed_update`].
    ///
    /// Equivalent to `interpolated_position(1.0)`; tests and tools use this to
    /// reason about the authoritative simulation state rather than the blended
    /// state that is drawn.
    #[must_use]
    pub fn position(&self) -> [f32; 2] {
        self.position
    }

    /// Advances the simulation by exactly `dt`.
    ///
    /// `direction` is a unit-length movement direction, `viewport` the drawable
    /// size in pixels. The square is kept fully inside the viewport.
    pub fn fixed_update(&mut self, direction: [f32; 2], dt: Duration, viewport: [f32; 2]) {
        self.previous = self.position;

        let distance = self.speed * dt.as_secs_f32();
        self.position[0] += direction[0] * distance;
        self.position[1] += direction[1] * distance;

        let half = self.size * 0.5;
        self.position[0] = clamp_center(self.position[0], half, viewport[0]);
        self.position[1] = clamp_center(self.position[1], half, viewport[1]);
    }

    /// Position to draw after `alpha` of the current fixed step has elapsed.
    ///
    /// Interpolating between the previous and current simulation states keeps
    /// motion smooth when the display refresh rate is not a multiple of the
    /// fixed update rate.
    #[must_use]
    pub fn interpolated_position(&self, alpha: f32) -> [f32; 2] {
        let alpha = alpha.clamp(0.0, 1.0);
        [
            self.previous[0] + (self.position[0] - self.previous[0]) * alpha,
            self.previous[1] + (self.position[1] - self.previous[1]) * alpha,
        ]
    }
}

/// Converts four "is this key held" flags into a direction of at most unit
/// length.
///
/// Ordering is `(left, right, up, down)`, matching the screen's y-down axis.
/// Normalizing keeps diagonal movement from being faster than axis-aligned
/// movement.
#[must_use]
pub fn direction_from_keys(axes: (bool, bool, bool, bool)) -> [f32; 2] {
    let (left, right, up, down) = axes;
    let x = f32::from(right) - f32::from(left);
    let y = f32::from(down) - f32::from(up);
    if x == 0.0 && y == 0.0 {
        return [0.0, 0.0];
    }
    let length = (x * x + y * y).sqrt();
    [x / length, y / length]
}

/// Keeps a center coordinate inside a viewport that is `extent` pixels wide.
///
/// A viewport smaller than the object collapses the object to the center
/// instead of panicking like `f32::clamp` would.
fn clamp_center(value: f32, half: f32, extent: f32) -> f32 {
    if extent <= half * 2.0 {
        extent * 0.5
    } else {
        value.clamp(half, extent - half)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: [f32; 2] = [800.0, 600.0];

    fn millis(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn movement_uses_delta_time() {
        let mut square = Square::new([400.0, 300.0], 64.0, 100.0);
        square.fixed_update([1.0, 0.0], millis(500), VIEWPORT);
        assert_eq!(square.position(), [450.0, 300.0], "100 px/s * 0.5 s");

        square.fixed_update([1.0, 0.0], millis(250), VIEWPORT);
        assert_eq!(
            square.position(),
            [475.0, 300.0],
            "delta time keeps scaling"
        );
    }

    #[test]
    fn diagonal_movement_is_not_faster() {
        let direction = direction_from_keys((false, true, false, true));
        let length = (direction[0] * direction[0] + direction[1] * direction[1]).sqrt();
        assert!((length - 1.0).abs() < 1e-6, "direction is unit length");

        let mut square = Square::new([400.0, 300.0], 64.0, 100.0);
        square.fixed_update(direction, millis(1000), VIEWPORT);
        let moved = [square.position()[0] - 400.0, square.position()[1] - 300.0];
        let distance = (moved[0] * moved[0] + moved[1] * moved[1]).sqrt();
        assert!((distance - 100.0).abs() < 1e-3, "moved {distance} px");
    }

    #[test]
    fn opposite_keys_cancel_out() {
        assert_eq!(direction_from_keys((true, true, true, true)), [0.0, 0.0]);
        assert_eq!(
            direction_from_keys((false, false, false, false)),
            [0.0, 0.0]
        );
        assert_eq!(
            direction_from_keys((true, false, false, false)),
            [-1.0, 0.0]
        );
        assert_eq!(
            direction_from_keys((false, false, true, false)),
            [0.0, -1.0]
        );
    }

    #[test]
    fn the_square_stays_inside_the_viewport() {
        let mut square = Square::new([50.0, 50.0], 64.0, 1000.0);
        // Walk far past every edge.
        square.fixed_update([-1.0, -1.0], millis(1000), VIEWPORT);
        assert_eq!(square.position(), [32.0, 32.0], "clamped to the top-left");

        square.fixed_update([1.0, 1.0], millis(1000), VIEWPORT);
        assert_eq!(
            square.position(),
            [768.0, 568.0],
            "clamped to the bottom-right"
        );
    }

    #[test]
    fn a_viewport_smaller_than_the_square_does_not_panic() {
        let mut square = Square::new([10.0, 10.0], 64.0, 100.0);
        square.fixed_update([1.0, 1.0], millis(100), [32.0, 32.0]);
        assert_eq!(square.position(), [16.0, 16.0]);
    }

    #[test]
    fn interpolation_blends_previous_and_current_positions() {
        // Start mid-viewport so the clamping applied by fixed_update does not
        // interfere with the arithmetic under test.
        let mut square = Square::new([400.0, 300.0], 32.0, 100.0);
        square.fixed_update([1.0, 0.0], millis(100), VIEWPORT);
        assert_eq!(square.interpolated_position(0.0)[0], 400.0);
        assert_eq!(square.interpolated_position(1.0)[0], 410.0);
        assert_eq!(square.interpolated_position(0.5)[0], 405.0);
        // Out-of-range alphas are clamped rather than extrapolated.
        assert_eq!(square.interpolated_position(2.0)[0], 410.0);
        assert_eq!(square.interpolated_position(-1.0)[0], 400.0);
    }

    #[test]
    fn interpolation_does_not_move_the_simulation() {
        let mut square = Square::new([100.0, 100.0], 32.0, 100.0);
        square.fixed_update([1.0, 0.0], millis(16), VIEWPORT);
        let before = square.position();
        let _ = square.interpolated_position(0.25);
        assert_eq!(square.position(), before);
    }
}
