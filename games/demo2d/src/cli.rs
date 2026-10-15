//! Command-line options for the demo.
//!
//! Hand-rolled rather than delegated to an argument-parsing crate: the demo has
//! a handful of options, and keeping the workspace dependency graph small is
//! worth more than the convenience here.

use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

/// Default abort timeout applied when `--frames` is used without `--timeout`.
pub const DEFAULT_FRAME_TIMEOUT: Duration = Duration::from_secs(30);

/// Zoom used when `--zoom` is not given.
pub const DEFAULT_ZOOM: f32 = 1.0;

/// How the demo drives its 2D camera.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    /// No camera: world coordinates are physical pixels (the default).
    #[default]
    None,
    /// The camera centers on the player every frame.
    Follow,
}

impl CameraMode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "follow" => Some(Self::Follow),
            _ => None,
        }
    }
}

/// Options the demo understands.
#[derive(Debug, PartialEq)]
pub struct Cli {
    /// Present this many frames and then exit cleanly.
    ///
    /// Intended for smoke tests: it makes a windowed application terminate on
    /// its own.
    pub frames: Option<u64>,
    /// Write the last presented frame to this path as a binary PPM image.
    pub capture: Option<PathBuf>,
    /// Print frame-time statistics when the demo exits.
    pub stats: bool,
    /// Abort if the requested frames are not presented within this duration.
    ///
    /// Only meaningful together with `--frames`.
    pub timeout: Option<Duration>,
    /// Camera to render through.
    pub camera: CameraMode,
    /// Camera zoom in physical pixels per world unit.
    pub zoom: f32,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            frames: None,
            capture: None,
            stats: false,
            timeout: None,
            camera: CameraMode::None,
            zoom: DEFAULT_ZOOM,
        }
    }
}

/// Why the command line could not be understood.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CliError {
    /// `--help` or `-h` was requested.
    #[error("help requested")]
    HelpRequested,
    /// An argument was not recognized.
    #[error("unknown argument '{0}'")]
    UnknownArgument(String),
    /// An option was given without its value.
    #[error("missing value for '{0}'")]
    MissingValue(String),
    /// A value could not be parsed.
    #[error("invalid value '{value}' for '{option}'")]
    InvalidValue {
        /// The option that received the value.
        option: String,
        /// The offending value.
        value: String,
    },
    /// An option was repeated.
    #[error("'{0}' was given more than once")]
    DuplicateArgument(String),
    /// `--zoom` was given without a camera to apply it to.
    #[error("'--zoom' requires '--camera' (for example: --camera follow --zoom 2)")]
    ZoomWithoutCamera,
}

impl Cli {
    /// Parses arguments, excluding the program name.
    ///
    /// # Errors
    ///
    /// Returns [`CliError`] for unknown options, missing or invalid values, and
    /// repeated options. [`CliError::HelpRequested`] is not a failure: the
    /// caller should print [`Cli::help`] and exit successfully.
    pub fn parse<I>(args: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut cli = Cli::default();
        let mut camera: Option<CameraMode> = None;
        let mut zoom: Option<f32> = None;
        let mut args = args.into_iter();

        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--help" | "-h" => return Err(CliError::HelpRequested),
                "--stats" => {
                    if cli.stats {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    cli.stats = true;
                }
                "--frames" => {
                    let value = args
                        .next()
                        .ok_or(CliError::MissingValue(argument.clone()))?;
                    if cli.frames.is_some() {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    let frames = value.parse::<u64>().map_err(|_| CliError::InvalidValue {
                        option: argument.clone(),
                        value: value.clone(),
                    })?;
                    if frames == 0 {
                        return Err(CliError::InvalidValue {
                            option: argument,
                            value,
                        });
                    }
                    cli.frames = Some(frames);
                }
                "--capture" => {
                    let value = args
                        .next()
                        .ok_or(CliError::MissingValue(argument.clone()))?;
                    if cli.capture.is_some() {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    cli.capture = Some(PathBuf::from(value));
                }
                "--timeout" => {
                    let value = args
                        .next()
                        .ok_or(CliError::MissingValue(argument.clone()))?;
                    if cli.timeout.is_some() {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    let seconds = value.parse::<u64>().map_err(|_| CliError::InvalidValue {
                        option: argument.clone(),
                        value: value.clone(),
                    })?;
                    cli.timeout = Some(Duration::from_secs(seconds));
                }
                "--camera" => {
                    let value = args
                        .next()
                        .ok_or(CliError::MissingValue(argument.clone()))?;
                    if camera.is_some() {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    let mode = CameraMode::parse(&value).ok_or_else(|| CliError::InvalidValue {
                        option: argument.clone(),
                        value: value.clone(),
                    })?;
                    camera = Some(mode);
                }
                "--zoom" => {
                    let value = args
                        .next()
                        .ok_or(CliError::MissingValue(argument.clone()))?;
                    if zoom.is_some() {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    let factor = value.parse::<f32>().map_err(|_| CliError::InvalidValue {
                        option: argument.clone(),
                        value: value.clone(),
                    })?;
                    // A zoom that is not a finite positive number cannot
                    // produce a meaningful view; reject it at the command line
                    // instead of rendering a broken frame.
                    if !factor.is_finite() || factor <= 0.0 {
                        return Err(CliError::InvalidValue {
                            option: argument,
                            value,
                        });
                    }
                    zoom = Some(factor);
                }
                other => return Err(CliError::UnknownArgument(other.to_string())),
            }
        }

        cli.camera = camera.unwrap_or_default();
        if zoom.is_some() && cli.camera == CameraMode::None {
            return Err(CliError::ZoomWithoutCamera);
        }
        cli.zoom = zoom.unwrap_or(DEFAULT_ZOOM);

        // A headless smoke test must not hang forever if the window never
        // presents the requested frames.
        if cli.frames.is_some() && cli.timeout.is_none() {
            cli.timeout = Some(DEFAULT_FRAME_TIMEOUT);
        }

        Ok(cli)
    }

    /// Usage text.
    #[must_use]
    pub const fn help() -> &'static str {
        "\
demo2d - Mikan 2D demo

USAGE:
    demo2d [OPTIONS]

OPTIONS:
    --frames <N>        Present N frames, then exit cleanly (smoke tests)
    --capture <PATH>    Write the last presented frame to PATH as a binary PPM
    --stats             Print frame-time statistics on exit
    --timeout <SECS>    Abort if N frames are not presented in time (default 30)
    --camera <MODE>     Camera to render through: none (default) or follow
    --zoom <FACTOR>     Pixels per world unit when a camera is active (default 1)
    -h, --help          Print this help

CONTROLS:
    WASD or arrow keys   Move the square
    Escape               Quit
    Close the window     Quit
"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, CliError> {
        Cli::parse(args.iter().map(|value| value.to_string()))
    }

    #[test]
    fn no_arguments_runs_interactively() {
        let cli = parse(&[]).expect("empty arguments are valid");
        assert_eq!(cli, Cli::default());
        assert!(cli.timeout.is_none(), "no frame budget, no timeout");
        assert_eq!(cli.camera, CameraMode::None);
        assert_eq!(cli.zoom, DEFAULT_ZOOM);
    }

    #[test]
    fn frames_imply_a_default_timeout() {
        let cli = parse(&["--frames", "120"]).expect("valid");
        assert_eq!(cli.frames, Some(120));
        assert_eq!(cli.timeout, Some(DEFAULT_FRAME_TIMEOUT));
    }

    #[test]
    fn explicit_timeout_overrides_the_default() {
        let cli = parse(&["--frames", "10", "--timeout", "5"]).expect("valid");
        assert_eq!(cli.timeout, Some(Duration::from_secs(5)));
    }

    #[test]
    fn capture_and_stats_are_recorded() {
        let cli = parse(&["--capture", "shot.ppm", "--stats"]).expect("valid");
        assert_eq!(cli.capture, Some(PathBuf::from("shot.ppm")));
        assert!(cli.stats);
    }

    #[test]
    fn the_camera_and_zoom_are_recorded_in_any_order() {
        let cli = parse(&["--zoom", "2.5", "--camera", "follow"]).expect("valid");
        assert_eq!(cli.camera, CameraMode::Follow);
        assert_eq!(cli.zoom, 2.5);

        let cli = parse(&["--camera", "none"]).expect("valid");
        assert_eq!(cli.camera, CameraMode::None);
        assert_eq!(cli.zoom, DEFAULT_ZOOM);

        let cli = parse(&["--camera", "follow"]).expect("valid");
        assert_eq!(cli.zoom, DEFAULT_ZOOM, "zoom defaults to 1:1");
    }

    #[test]
    fn zoom_without_a_camera_is_rejected() {
        assert_eq!(parse(&["--zoom", "2"]), Err(CliError::ZoomWithoutCamera));
        assert_eq!(
            parse(&["--camera", "none", "--zoom", "2"]),
            Err(CliError::ZoomWithoutCamera)
        );
    }

    #[test]
    fn invalid_camera_values_are_rejected_with_context() {
        assert_eq!(
            parse(&["--camera", "orbit"]),
            Err(CliError::InvalidValue {
                option: "--camera".to_string(),
                value: "orbit".to_string(),
            })
        );
        for value in ["0", "-1", "abc", "inf", "nan"] {
            assert_eq!(
                parse(&["--camera", "follow", "--zoom", value]),
                Err(CliError::InvalidValue {
                    option: "--zoom".to_string(),
                    value: value.to_string(),
                }),
                "zoom {value}"
            );
        }
    }

    #[test]
    fn help_is_reported_as_a_distinct_outcome() {
        assert_eq!(parse(&["--help"]), Err(CliError::HelpRequested));
        assert_eq!(parse(&["-h"]), Err(CliError::HelpRequested));
    }

    #[test]
    fn invalid_input_is_rejected_with_context() {
        assert_eq!(
            parse(&["--frames"]),
            Err(CliError::MissingValue("--frames".to_string()))
        );
        assert_eq!(
            parse(&["--frames", "many"]),
            Err(CliError::InvalidValue {
                option: "--frames".to_string(),
                value: "many".to_string(),
            })
        );
        assert_eq!(
            parse(&["--frames", "0"]),
            Err(CliError::InvalidValue {
                option: "--frames".to_string(),
                value: "0".to_string(),
            })
        );
        assert_eq!(
            parse(&["--wat"]),
            Err(CliError::UnknownArgument("--wat".to_string()))
        );
        assert_eq!(
            parse(&["--stats", "--stats"]),
            Err(CliError::DuplicateArgument("--stats".to_string()))
        );
        assert_eq!(
            parse(&["--camera", "follow", "--camera", "none"]),
            Err(CliError::DuplicateArgument("--camera".to_string()))
        );
        assert_eq!(
            parse(&["--camera", "follow", "--zoom", "2", "--zoom", "3"]),
            Err(CliError::DuplicateArgument("--zoom".to_string()))
        );
        assert_eq!(
            parse(&["--zoom"]),
            Err(CliError::MissingValue("--zoom".to_string()))
        );
    }
}
