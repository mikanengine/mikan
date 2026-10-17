//! Errors raised by the platform layer.

use thiserror::Error;

/// Failures that can occur while running an application.
///
/// Application callbacks return this type so that an application-provided error
/// can be reported through the same channel as a platform failure. Use
/// [`PlatformError::app`] to wrap application errors.
#[derive(Debug, Error)]
pub enum PlatformError {
    /// The event loop could not be created or run.
    #[error("event loop failure: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),

    /// The operating system refused to create the window.
    #[error("could not create the window: {0}")]
    WindowCreation(#[from] winit::error::OsError),

    /// The engine configuration was rejected before startup.
    #[error("invalid engine configuration: {0}")]
    Config(#[from] mikan_core::ConfigError),

    /// An application callback reported an error.
    #[error("application error: {0}")]
    App(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl PlatformError {
    /// Wraps an application error so it can travel through the runner.
    pub fn app(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::App(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Error)]
    #[error("synthetic failure")]
    struct Synthetic;

    #[test]
    fn application_errors_keep_their_source() {
        let error = PlatformError::app(Synthetic);
        assert!(error.to_string().contains("synthetic failure"));
        assert!(std::error::Error::source(&error).is_some());
    }
}
