//! Configuration shared by every engine layer.

use core::time::Duration;

use crate::error::ConfigError;

/// Default fixed update rate, in hertz.
pub const DEFAULT_FIXED_TIMESTEP_HZ: u32 = 120;

/// Default ceiling on fixed updates executed for a single frame.
///
/// This bounds the catch-up work after a long stall (debugger breakpoint,
/// window drag, laptop suspend) so that simulation can never spiral.
pub const DEFAULT_MAX_STEPS_PER_FRAME: u32 = 8;

/// Default ceiling applied to a single frame's delta before it is fed to the
/// fixed-timestep accumulator.
pub const DEFAULT_MAX_FRAME_DELTA: Duration = Duration::from_millis(250);

/// Default window size in physical pixels.
pub const DEFAULT_WINDOW_SIZE: (u32, u32) = (960, 600);

/// A linear RGBA color with components in the range `[0.0, 1.0]`.
///
/// Values are stored as `f32` and are used directly by the render layer. The
/// type deliberately does not depend on `wgpu`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Red component.
    pub r: f32,
    /// Green component.
    pub g: f32,
    /// Blue component.
    pub b: f32,
    /// Alpha component.
    pub a: f32,
}

impl Color {
    /// Opaque black.
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    /// Opaque white.
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);

    /// Creates a color from all four components.
    #[must_use]
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Creates an opaque color.
    #[must_use]
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self::new(r, g, b, 1.0)
    }

    /// Component array in RGBA order.
    #[must_use]
    pub const fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Returns `true` if every component is finite and within `[0.0, 1.0]`.
    #[must_use]
    pub fn is_valid(self) -> bool {
        let in_range = |c: f32| c.is_finite() && (0.0..=1.0).contains(&c);
        in_range(self.r) && in_range(self.g) && in_range(self.b) && in_range(self.a)
    }
}

impl Default for Color {
    /// A dark neutral background, suitable as a default clear color.
    fn default() -> Self {
        Self::rgb(0.05, 0.06, 0.08)
    }
}

/// Engine configuration consumed by the platform and render layers.
///
/// The same value is handed to every layer so the demo, the window, and the
/// swap chain are configured from one place. Call [`EngineConfig::validate`]
/// before using it; the platform runner does this for you.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Window title.
    pub title: String,
    /// Initial window width in physical pixels.
    pub window_width: u32,
    /// Initial window height in physical pixels.
    pub window_height: u32,
    /// Whether presentation should synchronize with the display refresh.
    pub vsync: bool,
    /// Fixed simulation updates per second.
    pub fixed_timestep_hz: u32,
    /// Maximum fixed updates executed per frame.
    pub max_steps_per_frame: u32,
    /// Maximum frame delta fed into the fixed-timestep accumulator.
    pub max_frame_delta: Duration,
    /// Color used to clear the framebuffer each frame.
    pub clear_color: Color,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            title: "Mikan".to_string(),
            window_width: DEFAULT_WINDOW_SIZE.0,
            window_height: DEFAULT_WINDOW_SIZE.1,
            vsync: true,
            fixed_timestep_hz: DEFAULT_FIXED_TIMESTEP_HZ,
            max_steps_per_frame: DEFAULT_MAX_STEPS_PER_FRAME,
            max_frame_delta: DEFAULT_MAX_FRAME_DELTA,
            clear_color: Color::default(),
        }
    }
}

impl EngineConfig {
    /// Validates the configuration, returning the first problem found.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if the window size is zero, the fixed timestep
    /// frequency is zero, the step budget is zero, or the clear color contains
    /// non-finite or out-of-range components.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.window_width == 0 || self.window_height == 0 {
            return Err(ConfigError::ZeroWindowSize {
                width: self.window_width,
                height: self.window_height,
            });
        }
        if self.fixed_timestep_hz == 0 {
            return Err(ConfigError::ZeroFixedTimestepHz);
        }
        if self.max_steps_per_frame == 0 {
            return Err(ConfigError::ZeroMaxStepsPerFrame);
        }
        if self.max_frame_delta.is_zero() {
            return Err(ConfigError::ZeroMaxFrameDelta);
        }
        if !self.clear_color.is_valid() {
            return Err(ConfigError::InvalidClearColor);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        assert!(EngineConfig::default().validate().is_ok());
    }

    #[test]
    fn zero_window_size_is_rejected() {
        let config = EngineConfig {
            window_width: 0,
            ..EngineConfig::default()
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ZeroWindowSize { .. })
        ));
    }

    #[test]
    fn zero_frequency_is_rejected() {
        let config = EngineConfig {
            fixed_timestep_hz: 0,
            ..EngineConfig::default()
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ZeroFixedTimestepHz)
        ));
    }

    #[test]
    fn zero_step_budget_is_rejected() {
        let config = EngineConfig {
            max_steps_per_frame: 0,
            ..EngineConfig::default()
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ZeroMaxStepsPerFrame)
        ));
    }

    #[test]
    fn invalid_clear_color_is_rejected() {
        for color in [
            Color::new(f32::NAN, 0.0, 0.0, 1.0),
            Color::new(0.0, 1.5, 0.0, 1.0),
            Color::new(0.0, 0.0, -0.1, 1.0),
            Color::new(0.0, 0.0, 0.0, f32::INFINITY),
        ] {
            assert!(!color.is_valid(), "{color:?} should be invalid");
            let config = EngineConfig {
                clear_color: color,
                ..EngineConfig::default()
            };
            assert!(matches!(
                config.validate(),
                Err(ConfigError::InvalidClearColor)
            ));
        }
    }

    #[test]
    fn color_bounds_are_inclusive() {
        assert!(Color::new(0.0, 0.0, 0.0, 0.0).is_valid());
        assert!(Color::new(1.0, 1.0, 1.0, 1.0).is_valid());
        assert_eq!(Color::WHITE.to_array(), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(Color::BLACK.to_array(), [0.0, 0.0, 0.0, 1.0]);
    }
}
