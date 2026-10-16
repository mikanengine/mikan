//! Mikan 2D demo.
//!
//! Opens a window and renders one of three scenes: the classic driveable
//! square, a feature showcase, or a sprite-stress scene. Assets are loaded from
//! `games/demo2d/assets` at startup. See `--help` for the smoke-test options.

mod assets;
mod cli;
mod demo;
mod player;
mod ppm;
mod scene;

use std::process::ExitCode;

use mikan::{EngineConfig, PlatformError, init_logging};
use thiserror::Error;

use crate::cli::{Cli, CliError};
use crate::demo::Demo;

/// Everything that can stop the demo before it exits cleanly.
#[derive(Debug, Error)]
enum RunError {
    #[error(transparent)]
    Cli(#[from] CliError),

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

    let mut config = EngineConfig {
        title: format!(
            "Mikan - Demo 2D ({}) - WASD to move, Esc to quit",
            cli.scene.name()
        ),
        vsync: cli.vsync,
        ..EngineConfig::default()
    };
    if let Some([width, height]) = cli.size {
        config.window_width = width;
        config.window_height = height;
    }

    log::info!(
        "starting demo2d: {}x{} window, {} Hz fixed updates, vsync {}",
        config.window_width,
        config.window_height,
        config.fixed_timestep_hz,
        config.vsync,
    );
    mikan::run(config, Demo::new(cli))?;
    Ok(())
}
