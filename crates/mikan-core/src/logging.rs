//! Logging initialization for engine binaries.

use std::sync::Once;

/// Filter used when `RUST_LOG` is not set.
pub const DEFAULT_LOG_FILTER: &str = "info";

/// Installs the process-wide logger, honoring the `RUST_LOG` environment
/// variable and falling back to [`DEFAULT_LOG_FILTER`].
///
/// This is safe to call more than once: initialization happens exactly once and
/// later calls are ignored. That makes it usable both from a `main` function
/// and from tests, where a harness may already have installed a logger.
pub fn init_logging() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let env = env_logger::Env::default().default_filter_or(DEFAULT_LOG_FILTER);
        let mut builder = env_logger::Builder::from_env(env);
        builder.format_timestamp_millis();
        // A logger installed by the embedding process (for example a test
        // harness) is not an error: our records still reach the same sink.
        let _ = builder.try_init();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_initialization_is_harmless() {
        // The second call must not panic; a naive `env_logger::init()` would.
        init_logging();
        init_logging();
    }
}
