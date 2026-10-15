//! Mikan 2D demo.
//!
//! Opens a window, renders a square that moves with the keyboard, and exits
//! cleanly when the window is closed. See `--help` for the smoke-test options.

mod cli;
mod demo;
mod ppm;
mod square;

use std::process::ExitCode;

use mikan_core::{EngineConfig, init_logging};
use mikan_platform::PlatformError;
use thiserror::Error;

use crate::cli::{Cli, CliError};
use crate::demo::Demo;

/// Everything that can stop the demo before it exits cleanly.
#[derive(Debug, Error)]
enum RunError {
    #[error(transparent)]
    Cli(#[from] CliError),

    #[error(transparent)]
    Config(#[from] mikan_core::ConfigError),

    #[error(transparent)]
    Platform(#[from] PlatformError),
}

fn main() -> ExitCode {
    init_logging();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Printed for a person watching the terminal; also logged so that
            // the failure appears in the same stream as engine diagnostics.
            eprintln!("demo2d: {error}");
            log::error!("demo2d exited with an error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), RunError> {
    let cli = match Cli::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(CliError::HelpRequested) => {
            print!("{}", Cli::help());
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };

    let config = EngineConfig {
        title: "Mikan - Demo 2D (WASD to move, Esc to quit)".to_string(),
        ..EngineConfig::default()
    };
    config.validate()?;

    log::info!(
        "starting demo2d: {}x{} window, {} Hz fixed updates",
        config.window_width,
        config.window_height,
        config.fixed_timestep_hz
    );
    mikan_platform::run(config, Demo::new(cli))?;
    Ok(())
}
