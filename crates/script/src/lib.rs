//! Sandboxed Luau gameplay scripting: one engine, one step contract.
//!
//! Gameplay decisions live in a Luau script; the engine owns everything else.
//! A [`ScriptEngine`] loads a script, checks that it defines the global step
//! function, and calls it once per fixed step with the facts a decision needs:
//!
//! ```lua
//! function mikan_step(dt, left, right, up, down, pos_x, pos_y)
//!     return dx, dy, speed -- a direction (any length) and pixels per second
//! end
//! ```
//!
//! The script returns three numbers; it cannot see or change the world, load
//! files, or reach the operating system. The VM is Luau (the same language
//! Roblox runs), built from vendored C++ sources by `mlua`'s `luau` and
//! `vendored` features, and sandboxed twice: only an explicit safe standard
//! library set is loaded (coroutine, table, string, math, utf8, bit32, and
//! Luau's side-effect-free `os` — no I/O, no package or module loading, no
//! `debug`), and Luau's own sandbox mode makes the global environment
//! read-only with a per-thread write proxy. A 64 MiB memory limit turns a
//! runaway script into a Lua error instead of an out-of-memory kill.
//!
//! The engine validates everything a script returns: all three numbers must be
//! finite and representable as `f32`, `speed` must not be negative, and a
//! direction longer than unit length is normalized. A shorter direction is
//! left alone, so a script can intentionally move slowly.
//!
//! Failure semantics (see ADR 0011):
//!
//! - **Load time** — an unreadable file, a compile error, or a script without
//!   `mikan_step` fails [`ScriptEngine::from_file`] / [`from_source`] with an
//!   error naming the script; applications decide whether that is fatal (the
//!   demo exits).
//! - **Run time** — an error raised inside `mikan_step`, or an invalid
//!   return, fails [`ScriptEngine::call_step`] for that step only. The engine
//!   has no opinion about what the game does next; the demo logs once and
//!   falls back to no input.
//!
//! [`from_source`]: ScriptEngine::from_source

use std::path::{Path, PathBuf};

use mlua::{Function, Lua, LuaOptions, StdLib};
use thiserror::Error;

/// Memory limit applied to every script VM, in bytes.
///
/// Scripts in this repository are small and trusted; the limit exists so that
/// a bug or an experiment cannot eat the process, and 64 MiB is far above
/// anything the step contract needs.
pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;

/// The facts a step decision is made from.
///
/// Mirrors the arguments of the script's `mikan_step` function; the script
/// never reads the world (or the clock) itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepInput {
    /// The fixed step duration, in seconds.
    pub dt: f32,
    /// Whether the left key is held.
    pub left: bool,
    /// Whether the right key is held.
    pub right: bool,
    /// Whether the up key is held.
    pub up: bool,
    /// Whether the down key is held.
    pub down: bool,
    /// The player's current position, in world pixels.
    pub position: [f32; 2],
}

impl StepInput {
    /// No keys held, zero time, origin: the base for struct-update literals
    /// and tests.
    pub const IDLE: Self = Self {
        dt: 0.0,
        left: false,
        right: false,
        up: false,
        down: false,
        position: [0.0, 0.0],
    };
}

/// The decision a step made: where the player wants to go and how fast.
///
/// Validated by the engine: both numbers are finite `f32`s, `speed` is
/// non-negative, and `direction` is zero or of length at most one.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StepOutput {
    /// Movement direction; length zero to one.
    pub direction: [f32; 2],
    /// Speed in pixels per second.
    pub speed: f32,
}

impl StepOutput {
    /// Stand still.
    pub const IDLE: Self = Self {
        direction: [0.0, 0.0],
        speed: 0.0,
    };
}

/// Everything that can go wrong with a script.
#[derive(Debug, Error)]
pub enum ScriptError {
    /// The script file could not be read.
    #[error("could not read script {path}: {source}")]
    Load {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The script could not be compiled, or its top-level code raised an
    /// error while loading.
    #[error("script {name} failed to load: {message}")]
    Compile {
        /// The script name (its path), as used in error messages.
        name: String,
        /// The compiler or top-level error, with file and line where Luau
        /// provides them.
        message: String,
    },

    /// The script does not define a global function `mikan_step`.
    #[error("script {name} does not define a global function `mikan_step`")]
    MissingFunction {
        /// The script name (its path).
        name: String,
    },

    /// `mikan_step` raised an error during a step.
    #[error("script {name} failed in mikan_step: {message}")]
    Runtime {
        /// The script name (its path).
        name: String,
        /// The error, with file and line where Luau provides them.
        message: String,
    },

    /// `mikan_step` returned values that cannot drive the simulation.
    #[error("script {name} returned an invalid step: {detail}")]
    InvalidReturn {
        /// The script name (its path).
        name: String,
        /// What was wrong with the return values.
        detail: String,
    },

    /// The script VM itself could not be configured.
    #[error("the script VM could not be configured: {message}")]
    Vm {
        /// The underlying error.
        message: String,
    },
}

/// A loaded, validated script, ready to be asked for one step at a time.
///
/// One engine drives one script. It owns the VM; call [`ScriptEngine::call_step`]
/// once per fixed step before the schedule runs. The engine is not `Sync`:
/// keep it on the thread that runs the game loop, which is where the fixed
/// step already lives.
pub struct ScriptEngine {
    lua: Lua,
    step: Function,
    name: String,
}

impl ScriptEngine {
    /// Loads and validates a script from a file.
    ///
    /// The file's path becomes the script's name in every later error message,
    /// so runtime errors point back at the file and line that raised them.
    ///
    /// # Errors
    ///
    /// Returns [`ScriptError::Load`] if the file cannot be read,
    /// [`ScriptError::Compile`] if it does not compile (or its top-level code
    /// fails), [`ScriptError::MissingFunction`] if it does not define
    /// `mikan_step`, and [`ScriptError::Vm`] if the VM cannot be configured.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ScriptError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(|source| ScriptError::Load {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_source(path.to_string_lossy().as_ref(), &source)
    }

    /// Loads and validates a script from an in-memory source.
    ///
    /// `name` is what error messages will call the script; the file variant
    /// uses the path. This constructor exists for tests, tooling, and
    /// anything that embeds scripts in the binary.
    ///
    /// # Errors
    ///
    /// As [`ScriptEngine::from_file`], minus [`ScriptError::Load`]: `name`
    /// represents an already-read source.
    pub fn from_source(name: &str, source: &str) -> Result<Self, ScriptError> {
        // An explicit library set rather than `Lua::new()`: mlua treats every
        // Luau standard library as safe, including `debug`, which gameplay
        // has no business with. Luau's `os` stays — it only has clock, date,
        // difftime, and time.
        let libs = StdLib::COROUTINE
            | StdLib::TABLE
            | StdLib::STRING
            | StdLib::MATH
            | StdLib::UTF8
            | StdLib::BIT
            | StdLib::OS;
        let lua = Lua::new_with(libs, LuaOptions::default()).map_err(|error| vm_error(&error))?;
        lua.sandbox(true).map_err(|error| vm_error(&error))?;
        lua.set_memory_limit(MEMORY_LIMIT)
            .map_err(|error| vm_error(&error))?;

        lua.load(source)
            .set_name(name)
            .exec()
            .map_err(|error| ScriptError::Compile {
                name: name.to_string(),
                message: error.to_string(),
            })?;

        let step: Function =
            lua.globals()
                .get("mikan_step")
                .map_err(|_| ScriptError::MissingFunction {
                    name: name.to_string(),
                })?;

        Ok(Self {
            lua,
            step,
            name: name.to_string(),
        })
    }

    /// The name this script reports in errors (its path, when loaded from a
    /// file).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The number of bytes the script VM currently holds.
    ///
    /// This observes the [`MEMORY_LIMIT`] headroom while tuning a script. It
    /// includes the VM's own bookkeeping, so it is a diagnostic, not an exact
    /// accounting of script-owned data.
    #[must_use]
    pub fn used_memory(&self) -> usize {
        self.lua.used_memory()
    }

    /// Calls `mikan_step` and validates what it returned.
    ///
    /// # Errors
    ///
    /// Returns [`ScriptError::Runtime`] if the script raises an error, and
    /// [`ScriptError::InvalidReturn`] if the returned numbers are missing,
    /// not numbers, non-finite (in `f32`), or include a negative `speed`.
    pub fn call_step(&self, input: StepInput) -> Result<StepOutput, ScriptError> {
        let (dx, dy, speed): (f64, f64, f64) = self
            .step
            .call((
                f64::from(input.dt),
                input.left,
                input.right,
                input.up,
                input.down,
                f64::from(input.position[0]),
                f64::from(input.position[1]),
            ))
            .map_err(|error| match error {
                mlua::Error::FromLuaConversionError { .. } => ScriptError::InvalidReturn {
                    name: self.name.clone(),
                    detail: format!("the step must return three numbers: {error}"),
                },
                other => ScriptError::Runtime {
                    name: self.name.clone(),
                    message: other.to_string(),
                },
            })?;

        // The simulation works in f32, so that is the domain the values must
        // be valid in; a value that overflows f32 is a bug, not a clip.
        let direction = [dx as f32, dy as f32];
        let speed = speed as f32;
        if !direction[0].is_finite() || !direction[1].is_finite() || !speed.is_finite() {
            return Err(ScriptError::InvalidReturn {
                name: self.name.clone(),
                detail: format!(
                    "the step returned a non-finite value: dx = {dx}, dy = {dy}, speed = {speed}"
                ),
            });
        }
        if speed < 0.0 {
            return Err(ScriptError::InvalidReturn {
                name: self.name.clone(),
                detail: format!("the step returned a negative speed: {speed}"),
            });
        }

        let length_squared = direction[0] * direction[0] + direction[1] * direction[1];
        let direction = if length_squared > 1.0 {
            let length = length_squared.sqrt();
            [direction[0] / length, direction[1] / length]
        } else {
            direction
        };

        Ok(StepOutput { direction, speed })
    }
}

fn vm_error(error: &mlua::Error) -> ScriptError {
    ScriptError::Vm {
        message: error.to_string(),
    }
}

impl std::fmt::Debug for ScriptEngine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScriptEngine")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(source: &str) -> ScriptEngine {
        ScriptEngine::from_source("test.luau", source).expect("the fixture script must load")
    }

    #[test]
    fn a_valid_step_returns_direction_and_speed() {
        let engine = engine(
            r#"
            function mikan_step(dt, left, right, up, down, pos_x, pos_y)
                if right then return 1, 0, 60 end
                return 0, 0, 0
            end
            "#,
        );
        let moving = engine
            .call_step(StepInput {
                right: true,
                ..StepInput::IDLE
            })
            .unwrap();
        assert_eq!(
            moving,
            StepOutput {
                direction: [1.0, 0.0],
                speed: 60.0
            }
        );
        let idle = engine.call_step(StepInput::IDLE).unwrap();
        assert_eq!(idle, StepOutput::IDLE);
    }

    #[test]
    fn the_engine_can_be_asked_for_step_after_step() {
        let engine = engine(
            r#"
            function mikan_step(dt, left, right, up, down, pos_x, pos_y)
                if pos_x > 500 then return 0, 0, 0 end
                return 1, 0, 10
            end
            "#,
        );
        // The script's decisions depend only on its arguments, and every call
        // sees the current ones.
        let first = engine
            .call_step(StepInput {
                position: [480.0, 300.0],
                ..StepInput::IDLE
            })
            .unwrap();
        let second = engine
            .call_step(StepInput {
                position: [501.0, 300.0],
                ..StepInput::IDLE
            })
            .unwrap();
        assert_eq!(first.direction, [1.0, 0.0]);
        assert_eq!(second, StepOutput::IDLE);
    }

    #[test]
    fn a_short_direction_is_left_alone() {
        let engine = engine(
            r#"
            function mikan_step(dt, left, right, up, down, pos_x, pos_y)
                return 0.25, 0, 10
            end
            "#,
        );
        let output = engine.call_step(StepInput::IDLE).unwrap();
        assert_eq!(output.direction, [0.25, 0.0], "slow walks stay slow");
    }

    #[test]
    fn an_over_long_direction_is_normalized() {
        let engine = engine(
            r#"
            function mikan_step(dt, left, right, up, down, pos_x, pos_y)
                return 3, 4, 60
            end
            "#,
        );
        let output = engine.call_step(StepInput::IDLE).unwrap();
        assert!((output.direction[0] - 0.6).abs() < 1e-6);
        assert!((output.direction[1] - 0.8).abs() < 1e-6);
        assert_eq!(output.speed, 60.0);
    }

    #[test]
    fn a_non_finite_return_is_rejected() {
        for bad in ["0/0", "1/0", "-1/0"] {
            let engine = engine(&format!(
                r#"
                function mikan_step(dt, left, right, up, down, pos_x, pos_y)
                    return {bad}, 0, 1
                end
                "#,
            ));
            let error = engine.call_step(StepInput::IDLE).unwrap_err();
            assert!(
                matches!(error, ScriptError::InvalidReturn { .. }),
                "{bad} must be rejected, got: {error}"
            );
        }
    }

    #[test]
    fn a_non_finite_speed_is_rejected() {
        let engine = engine(
            r#"
            function mikan_step(dt, left, right, up, down, pos_x, pos_y)
                return 0, 0, 1/0
            end
            "#,
        );
        let error = engine.call_step(StepInput::IDLE).unwrap_err();
        assert!(matches!(error, ScriptError::InvalidReturn { .. }));
    }

    #[test]
    fn a_negative_speed_is_rejected() {
        let engine = engine(
            r#"
            function mikan_step(dt, left, right, up, down, pos_x, pos_y)
                return 1, 0, -30
            end
            "#,
        );
        let error = engine.call_step(StepInput::IDLE).unwrap_err();
        assert!(matches!(error, ScriptError::InvalidReturn { .. }));
    }

    #[test]
    fn a_wrong_return_count_is_rejected() {
        let engine = engine(
            r#"
            function mikan_step(dt, left, right, up, down, pos_x, pos_y)
                return 1, 0
            end
            "#,
        );
        let error = engine.call_step(StepInput::IDLE).unwrap_err();
        assert!(
            matches!(error, ScriptError::InvalidReturn { .. }),
            "got: {error}"
        );
    }

    #[test]
    fn a_wrong_return_type_is_rejected() {
        let engine = engine(
            r#"
            function mikan_step(dt, left, right, up, down, pos_x, pos_y)
                return "left", "right", "fast"
            end
            "#,
        );
        let error = engine.call_step(StepInput::IDLE).unwrap_err();
        assert!(
            matches!(error, ScriptError::InvalidReturn { .. }),
            "got: {error}"
        );
    }

    #[test]
    fn a_runtime_error_names_the_script_and_the_line() {
        let engine = engine(
            "function mikan_step(dt, left, right, up, down, pos_x, pos_y)\n    error(\"boom\")\nend\n",
        );
        let error = engine.call_step(StepInput::IDLE).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("boom"), "got: {message}");
        assert!(message.contains("test.luau"), "got: {message}");
        assert!(message.contains(":2"), "line 2 raised it, got: {message}");
    }

    #[test]
    fn a_compile_error_names_the_script_and_the_line() {
        let error = ScriptEngine::from_source("broken.luau", "function mikan_step(\n").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("broken.luau"), "got: {message}");
        assert!(message.contains("syntax error"), "got: {message}");
        // What the contract promises is name-and-line reporting; the exact
        // line is the compiler's business (a truncated source breaks at the
        // end). A line number is a colon followed by a digit.
        let has_line = message
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0] == b':' && pair[1].is_ascii_digit());
        assert!(has_line, "no line number in: {message}");
    }

    #[test]
    fn a_script_without_the_step_function_is_rejected() {
        let error = ScriptEngine::from_source("idle.luau", "local x = 1\n").unwrap_err();
        assert!(
            matches!(error, ScriptError::MissingFunction { ref name } if name == "idle.luau"),
            "got: {error}"
        );
    }

    #[test]
    fn a_global_that_is_not_a_function_is_rejected() {
        let error = ScriptEngine::from_source("fake.luau", "mikan_step = 42\n").unwrap_err();
        assert!(
            matches!(error, ScriptError::MissingFunction { .. }),
            "got: {error}"
        );
    }

    #[test]
    fn from_file_reports_a_missing_file() {
        let error = ScriptEngine::from_file("/nonexistent/player.luau").expect_err("no such file");
        assert!(
            matches!(&error, ScriptError::Load { path, .. } if path.ends_with("player.luau")),
            "got: {error}"
        );
    }

    #[test]
    fn from_file_uses_the_path_as_the_script_name() {
        let path = std::env::temp_dir().join(format!(
            "mikan-script-{}-name-test.luau",
            std::process::id()
        ));
        std::fs::write(
            &path,
            "function mikan_step(dt, left, right, up, down, pos_x, pos_y)\n    error(\"from file\")\nend\n",
        )
        .expect("the temp file must be writable");

        let engine = ScriptEngine::from_file(&path).expect("the temp script must load");
        assert_eq!(engine.name(), path.to_string_lossy().as_ref());
        let error = engine.call_step(StepInput::IDLE).unwrap_err();
        assert!(error.to_string().contains("from file"), "got: {error}");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_sandbox_hides_io_debug_and_file_loading() {
        // The explicit library set keeps Luau's `os` (clock, date, difftime,
        // time — no process control) and does not load `require` at all
        // (there is no package library), so the pcall below fails either way.
        // What must not exist is any way to touch the filesystem, other
        // processes, module loading, or native code.
        let engine = engine(
            r#"
            function mikan_step(dt, left, right, up, down, pos_x, pos_y)
                local can_escape =
                    io ~= nil
                    or package ~= nil
                    or debug ~= nil
                    or loadfile ~= nil
                    or dofile ~= nil
                    or (os ~= nil and (os.exit ~= nil or os.execute ~= nil or os.getenv ~= nil))
                    or pcall(require, "definitely-not-a-real-module")
                if can_escape then
                    return 1, 1, 1
                end
                return 0, 0, 0
            end
            "#,
        );
        let output = engine.call_step(StepInput::IDLE).unwrap();
        assert_eq!(
            output,
            StepOutput::IDLE,
            "the sandbox must not expose io, package, debug, loadfile, dofile, \
             process control, or module loading"
        );
    }

    #[test]
    fn the_memory_limit_stops_runaway_allocation() {
        // Distinct strings: Luau interns identical ones, so a runaway test
        // must build different data to consume memory.
        let error = ScriptEngine::from_source(
            "greedy.luau",
            r#"
            local blocks = {}
            for i = 1, 1024 do
                blocks[i] = string.rep(tostring(i), 64 * 1024)
            end
            function mikan_step(dt, left, right, up, down, pos_x, pos_y)
                return 0, 0, 0
            end
            "#,
        )
        .expect_err("more than 64 MiB of live strings must trip the limit");
        let message = error.to_string();
        assert!(message.to_lowercase().contains("memory"), "got: {message}");
    }

    #[test]
    fn the_vm_reports_its_memory_use() {
        let engine = ScriptEngine::from_source(
            "small.luau",
            "function mikan_step(dt, left, right, up, down, pos_x, pos_y)\n    return 0, 0, 0\nend",
        )
        .expect("a valid script loads");
        assert!(engine.used_memory() > 0, "a configured VM uses memory");
        assert!(
            engine.used_memory() < MEMORY_LIMIT,
            "a small script leaves headroom under the {} byte limit, but uses {} bytes",
            MEMORY_LIMIT,
            engine.used_memory()
        );
    }
}
