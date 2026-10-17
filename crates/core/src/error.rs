//! Engine-wide error types that do not belong to a single subsystem.

use thiserror::Error;

/// Problems detected in an [`EngineConfig`](crate::EngineConfig).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    /// The configured window size has a zero dimension.
    #[error("window size must be greater than zero, got {width}x{height}")]
    ZeroWindowSize {
        /// Configured width in physical pixels.
        width: u32,
        /// Configured height in physical pixels.
        height: u32,
    },
    /// The fixed update rate was set to zero.
    #[error("fixed timestep frequency must be greater than zero")]
    ZeroFixedTimestepHz,
    /// The per-frame fixed update budget was set to zero.
    #[error("max steps per frame must be greater than zero")]
    ZeroMaxStepsPerFrame,
    /// The maximum frame delta was set to zero.
    #[error("max frame delta must be greater than zero")]
    ZeroMaxFrameDelta,
    /// The clear color contains a non-finite or out-of-range component.
    #[error("clear color components must be finite and within [0.0, 1.0]")]
    InvalidClearColor,
}
