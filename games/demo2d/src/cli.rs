//! Command-line options for the demo.
//!
//! Hand-rolled rather than delegated to an argument-parsing crate: the demo has
//! five options, and keeping the workspace dependency graph small is worth more
//! than the convenience here.

use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

/// Default abort timeout applied when `--frames` is used without `--timeout`.
pub const DEFAULT_FRAME_TIMEOUT: Duration = Duration::from_secs(30);

/// Options the demo understands.
#[derive(Debug, Default, PartialEq, Eq)]
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
                other => return Err(CliError::UnknownArgument(other.to_string())),
            }
        }

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
    --frames <N>       Present N frames, then exit cleanly (smoke tests)
    --capture <PATH>   Write the last presented frame to PATH as a binary PPM
    --stats            Print frame-time statistics on exit
    --timeout <SECS>   Abort if N frames are not presented in time (default 30)
    -h, --help         Print this help

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
    }
}
