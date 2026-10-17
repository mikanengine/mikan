//! Command-line options for the demo.
//!
//! Hand-rolled rather than delegated to an argument-parsing crate: the demo has
//! a handful of options, and keeping the workspace dependency graph small is
//! worth more than the convenience here.

use std::path::PathBuf;
use std::time::Duration;

use mikan::SamplerMode;
use thiserror::Error;

/// Default abort timeout applied when `--frames` is used without `--timeout`.
pub const DEFAULT_FRAME_TIMEOUT: Duration = Duration::from_secs(30);

/// Zoom used when `--zoom` is not given.
pub const DEFAULT_ZOOM: f32 = 1.0;

/// Directory loaded when `--assets` is not given, relative to the working
/// directory (the repository root).
pub const DEFAULT_ASSETS_DIR: &str = "games/demo2d/assets";

/// Gameplay script loaded when `--script` is not given, relative to the
/// working directory (the repository root).
pub const DEFAULT_SCRIPT: &str = "games/demo2d/scripts/player.luau";

/// Sprites drawn by the stress scene when `--sprites` is not given.
pub const DEFAULT_STRESS_SPRITES: u32 = 10_000;

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

/// Which scene the demo draws.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    /// The original demo: a driveable square and a corner marker.
    #[default]
    Classic,
    /// A sun, orbiting planets, and a moon linked by the transform hierarchy.
    Hierarchy,
    /// A fixed gallery: rotation, tint, alpha, nearest vs linear, and the
    /// transparent star, each at a documented position.
    Showcase,
    /// `--sprites` moving sprites from one texture, for benchmarks.
    Stress,
}

impl Scene {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "classic" => Some(Self::Classic),
            "hierarchy" => Some(Self::Hierarchy),
            "showcase" => Some(Self::Showcase),
            "stress" => Some(Self::Stress),
            _ => None,
        }
    }

    /// The scene's command-line name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Hierarchy => "hierarchy",
            Self::Showcase => "showcase",
            Self::Stress => "stress",
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
    /// Scene to draw.
    pub scene: Scene,
    /// Sampler applied to every sprite the scene does not deliberately pin;
    /// `None` keeps each scene's default (linear).
    pub sampler: Option<SamplerMode>,
    /// Pixel-perfect mode: the virtual screen size, in virtual pixels.
    pub pixel_perfect: Option<[u32; 2]>,
    /// Window size in physical pixels; `None` keeps the engine default.
    pub size: Option<[u32; 2]>,
    /// Present with vsync; `--no-vsync` clears it for uncapped benchmarking.
    pub vsync: bool,
    /// Directory holding `atlas.json`, `atlas.png`, and `star.png`.
    pub assets: PathBuf,
    /// Luau gameplay script driving the player's movement.
    pub script: PathBuf,
    /// Sprite count of the stress scene; `None` uses the default.
    pub sprites: Option<u32>,
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
            scene: Scene::default(),
            sampler: None,
            pixel_perfect: None,
            size: None,
            vsync: true,
            assets: PathBuf::from(DEFAULT_ASSETS_DIR),
            script: PathBuf::from(DEFAULT_SCRIPT),
            sprites: None,
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
    /// `--zoom` and `--pixel-perfect` both set a scale.
    #[error(
        "'--zoom' cannot be combined with '--pixel-perfect': the integer zoom is derived from the virtual resolution"
    )]
    PixelPerfectWithZoom,
    /// `--sprites` was given for a scene that has no sprite count.
    #[error("'--sprites' only applies to '--scene stress'")]
    SpritesWithoutStress,
}

impl Cli {
    /// Parses arguments, excluding the program name.
    ///
    /// # Errors
    ///
    /// Returns [`CliError`] for unknown options, missing or invalid values,
    /// repeated options, and option combinations that contradict each other.
    /// [`CliError::HelpRequested`] is not a failure: the caller should print
    /// [`Cli::help`] and exit successfully.
    pub fn parse<I>(args: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut cli = Cli::default();
        let mut camera: Option<CameraMode> = None;
        let mut zoom: Option<f32> = None;
        let mut scene: Option<Scene> = None;
        let mut sampler: Option<SamplerMode> = None;
        let mut pixel_perfect: Option<[u32; 2]> = None;
        let mut size: Option<[u32; 2]> = None;
        let mut no_vsync = false;
        let mut assets: Option<PathBuf> = None;
        let mut script: Option<PathBuf> = None;
        let mut sprites: Option<u32> = None;
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
                "--no-vsync" => {
                    if no_vsync {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    no_vsync = true;
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
                "--assets" => {
                    let value = args
                        .next()
                        .ok_or(CliError::MissingValue(argument.clone()))?;
                    if assets.is_some() {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    assets = Some(PathBuf::from(value));
                }
                "--script" => {
                    let value = args
                        .next()
                        .ok_or(CliError::MissingValue(argument.clone()))?;
                    if script.is_some() {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    script = Some(PathBuf::from(value));
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
                "--sprites" => {
                    let value = args
                        .next()
                        .ok_or(CliError::MissingValue(argument.clone()))?;
                    if sprites.is_some() {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    let count = value.parse::<u32>().map_err(|_| CliError::InvalidValue {
                        option: argument.clone(),
                        value: value.clone(),
                    })?;
                    if count == 0 {
                        return Err(CliError::InvalidValue {
                            option: argument,
                            value,
                        });
                    }
                    sprites = Some(count);
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
                "--scene" => {
                    let value = args
                        .next()
                        .ok_or(CliError::MissingValue(argument.clone()))?;
                    if scene.is_some() {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    let parsed = Scene::parse(&value).ok_or_else(|| CliError::InvalidValue {
                        option: argument.clone(),
                        value: value.clone(),
                    })?;
                    scene = Some(parsed);
                }
                "--sampler" => {
                    let value = args
                        .next()
                        .ok_or(CliError::MissingValue(argument.clone()))?;
                    if sampler.is_some() {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    let mode = match value.as_str() {
                        "linear" => SamplerMode::Linear,
                        "nearest" => SamplerMode::Nearest,
                        _ => {
                            return Err(CliError::InvalidValue {
                                option: argument,
                                value,
                            });
                        }
                    };
                    sampler = Some(mode);
                }
                "--pixel-perfect" => {
                    let value = args
                        .next()
                        .ok_or(CliError::MissingValue(argument.clone()))?;
                    if pixel_perfect.is_some() {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    pixel_perfect = Some(parse_size(&argument, &value)?);
                }
                "--size" => {
                    let value = args
                        .next()
                        .ok_or(CliError::MissingValue(argument.clone()))?;
                    if size.is_some() {
                        return Err(CliError::DuplicateArgument(argument));
                    }
                    size = Some(parse_size(&argument, &value)?);
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
        if zoom.is_some() && pixel_perfect.is_some() {
            return Err(CliError::PixelPerfectWithZoom);
        }
        cli.zoom = zoom.unwrap_or(DEFAULT_ZOOM);
        cli.scene = scene.unwrap_or_default();
        cli.sampler = sampler;
        cli.pixel_perfect = pixel_perfect;
        cli.size = size;
        cli.vsync = !no_vsync;
        cli.assets = assets.unwrap_or_else(|| PathBuf::from(DEFAULT_ASSETS_DIR));
        cli.script = script.unwrap_or_else(|| PathBuf::from(DEFAULT_SCRIPT));
        if sprites.is_some() && cli.scene != Scene::Stress {
            return Err(CliError::SpritesWithoutStress);
        }
        cli.sprites = sprites;

        // A headless smoke test must not hang forever if the window never
        // presents the requested frames.
        if cli.frames.is_some() && cli.timeout.is_none() {
            cli.timeout = Some(DEFAULT_FRAME_TIMEOUT);
        }

        Ok(cli)
    }

    /// Sprite count the stress scene will draw.
    #[must_use]
    pub fn stress_sprites(&self) -> u32 {
        self.sprites.unwrap_or(DEFAULT_STRESS_SPRITES)
    }

    /// Usage text.
    #[must_use]
    pub const fn help() -> &'static str {
        "\
demo2d - Mikan 2D demo

USAGE:
    demo2d [OPTIONS]

OPTIONS:
    --frames <N>          Present N frames, then exit cleanly (smoke tests)
    --capture <PATH>      Write the last presented frame to PATH as a binary PPM
    --stats               Print frame-time and draw statistics on exit
    --timeout <SECS>      Abort if N frames are not presented in time (default 30)
    --scene <NAME>        classic (default), hierarchy, showcase, or stress
    --sampler <MODE>      Force linear or nearest sampling scene-wide
    --assets <DIR>        Asset directory (default games/demo2d/assets)
    --script <PATH>       Luau gameplay script (default games/demo2d/scripts/player.luau)
    --size <WxH>          Window size in pixels (default 960x600)
    --camera <MODE>       Camera to render through: none (default) or follow
    --zoom <FACTOR>       Pixels per world unit when a camera is active (default 1)
    --pixel-perfect <WxH> Integer-zoom pixel-perfect camera for a WxH virtual screen
    --no-vsync            Present uncapped, for benchmarking
    --sprites <N>         Sprite count of the stress scene (default 10000)
    -h, --help            Print this help

SCENES:
    classic     The driveable square and corner marker, loaded from atlas.png
    hierarchy   A sun, three orbiting planets, and a moon, linked as entities
    showcase    Rotation, tint, alpha, nearest vs linear, and star.png alpha
    stress      N moving sprites drawn from one texture, for benchmarks

CONTROLS:
    WASD or arrow keys   Move the square (classic)
    A / D                Spin the sun, orbiting the hierarchy scene
    F                    Despawn the moon (hierarchy)
    Escape               Quit
    Close the window     Quit
"
    }
}

/// Parses a `WxH` size, rejecting missing or zero dimensions.
fn parse_size(argument: &str, value: &str) -> Result<[u32; 2], CliError> {
    let invalid = || CliError::InvalidValue {
        option: argument.to_string(),
        value: value.to_string(),
    };
    let (width, height) = value.split_once(['x', 'X']).ok_or_else(invalid)?;
    let width = width.parse::<u32>().map_err(|_| invalid())?;
    let height = height.parse::<u32>().map_err(|_| invalid())?;
    if width == 0 || height == 0 {
        return Err(invalid());
    }
    Ok([width, height])
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
        assert_eq!(cli.scene, Scene::Classic);
        assert_eq!(cli.sampler, None);
        assert_eq!(cli.pixel_perfect, None);
        assert_eq!(cli.size, None);
        assert!(cli.vsync);
        assert_eq!(cli.assets, PathBuf::from(DEFAULT_ASSETS_DIR));
        assert_eq!(cli.script, PathBuf::from(DEFAULT_SCRIPT));
        assert_eq!(cli.stress_sprites(), DEFAULT_STRESS_SPRITES);
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
    fn scenes_samplers_and_flags_are_recorded() {
        let cli = parse(&[
            "--scene",
            "showcase",
            "--sampler",
            "nearest",
            "--no-vsync",
            "--size",
            "800x600",
            "--assets",
            "somewhere/assets",
        ])
        .expect("valid");
        assert_eq!(cli.scene, Scene::Showcase);
        assert_eq!(cli.sampler, Some(SamplerMode::Nearest));
        assert!(!cli.vsync);
        assert_eq!(cli.size, Some([800, 600]));
        assert_eq!(cli.assets, PathBuf::from("somewhere/assets"));

        let cli = parse(&[
            "--sampler",
            "linear",
            "--scene",
            "stress",
            "--sprites",
            "25000",
        ])
        .expect("valid");
        assert_eq!(cli.sampler, Some(SamplerMode::Linear));
        assert_eq!(cli.scene, Scene::Stress);
        assert_eq!(cli.stress_sprites(), 25_000);
    }

    #[test]
    fn pixel_perfect_is_recorded_and_conflicts_with_zoom() {
        let cli = parse(&["--pixel-perfect", "320x200"]).expect("valid");
        assert_eq!(cli.pixel_perfect, Some([320, 200]));

        let cli = parse(&["--camera", "follow", "--pixel-perfect", "240X150"]).expect("valid");
        assert_eq!(cli.pixel_perfect, Some([240, 150]));
        assert_eq!(cli.camera, CameraMode::Follow);

        assert_eq!(
            parse(&[
                "--camera",
                "follow",
                "--zoom",
                "2",
                "--pixel-perfect",
                "320x200"
            ]),
            Err(CliError::PixelPerfectWithZoom)
        );
    }

    #[test]
    fn malformed_sizes_are_rejected_with_context() {
        for value in ["960", "0x600", "960x0", "axb", "960x", "x600", "-960x600"] {
            assert_eq!(
                parse(&["--size", value]),
                Err(CliError::InvalidValue {
                    option: "--size".to_string(),
                    value: value.to_string(),
                }),
                "size {value}"
            );
            assert_eq!(
                parse(&["--pixel-perfect", value]),
                Err(CliError::InvalidValue {
                    option: "--pixel-perfect".to_string(),
                    value: value.to_string(),
                }),
                "pixel-perfect {value}"
            );
        }
        assert_eq!(
            parse(&["--scene", "stress", "--sprites", "0"]),
            Err(CliError::InvalidValue {
                option: "--sprites".to_string(),
                value: "0".to_string(),
            })
        );
    }

    #[test]
    fn sprites_require_the_stress_scene() {
        assert_eq!(
            parse(&["--sprites", "100"]),
            Err(CliError::SpritesWithoutStress)
        );
        assert_eq!(
            parse(&["--scene", "showcase", "--sprites", "100"]),
            Err(CliError::SpritesWithoutStress)
        );
        assert!(parse(&["--scene", "stress", "--sprites", "100"]).is_ok());
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
        assert_eq!(
            parse(&["--scene", "stressy"]),
            Err(CliError::InvalidValue {
                option: "--scene".to_string(),
                value: "stressy".to_string(),
            })
        );
        assert_eq!(
            parse(&["--sampler", "smooth"]),
            Err(CliError::InvalidValue {
                option: "--sampler".to_string(),
                value: "smooth".to_string(),
            })
        );
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
            parse(&["--no-vsync", "--no-vsync"]),
            Err(CliError::DuplicateArgument("--no-vsync".to_string()))
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
        assert_eq!(
            parse(&["--scene", "classic", "--scene", "stress"]),
            Err(CliError::DuplicateArgument("--scene".to_string()))
        );
        assert_eq!(
            parse(&["--assets", "a", "--assets", "b"]),
            Err(CliError::DuplicateArgument("--assets".to_string()))
        );
        assert_eq!(
            parse(&["--script", "a.luau", "--script", "b.luau"]),
            Err(CliError::DuplicateArgument("--script".to_string()))
        );
        assert_eq!(
            parse(&["--script"]),
            Err(CliError::MissingValue("--script".to_string()))
        );
        assert_eq!(
            parse(&["--sprites"]),
            Err(CliError::MissingValue("--sprites".to_string()))
        );
    }
}
