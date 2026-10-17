# Gameplay scripting (Luau)

Mikan embeds **Luau** — the statically-optimized Lua derivative Roblox runs —
as the gameplay scripting language. A script decides *intent*; the engine owns
everything else. This guide describes the contract, the sandbox, the failure
semantics, and how the demo integrates both. The decision record is
[ADR 0011](adr/0011-luau-gameplay-scripting.md); the C++ role (the vendored
Luau VM) is stated there and in `docs/ffi.md`.

## The crate

`crates/script` is safe Rust (no `unsafe`). It builds Luau from the
sources vendored through `mlua`'s `luau` and `vendored` features, so no system
Lua installation is required — but a **C++ compiler is** (the Luau VM is C++
sources compiled by `mlua-sys`'s build script).

Public API:

| Item | Meaning |
| --- | --- |
| `ScriptEngine::from_file(path)` | Loads, compiles, and validates a script. The path becomes the script name in every later error message. |
| `ScriptEngine::from_source(name, source)` | The same from memory (tests, tooling, embedded scripts). |
| `ScriptEngine::call_step(input)` | Calls `mikan_step` once and validates what it returned. |
| `ScriptEngine::used_memory()` | The VM's current memory use in bytes, for observing the 64 MiB limit's headroom. |
| `StepInput { dt, left, right, up, down, position }` | The facts a decision is made from. |
| `StepOutput { direction, speed }` | The validated decision. |
| `ScriptError` | `Load`, `Compile`, `MissingFunction`, `Runtime`, `InvalidReturn`, `Vm` — each naming the script and, where Luau provides it, the line. |

One engine drives one script; keep it on the game-loop thread (it is not
`Sync`), which is where the fixed step already lives.

## The step contract

```lua
function mikan_step(dt, left, right, up, down, pos_x, pos_y)
    return dx, dy, speed -- a direction and pixels per second
end
```

- `dt` — the fixed step duration in seconds (a double).
- `left, right, up, down` — booleans: are the keys held this step?
- `pos_x, pos_y` — the player's position in world pixels.
- The return is **three numbers**: a direction (any length) and a speed in
  pixels per second.

The script can also define helpers, constants, and upvalues around the
function — the chunk runs once at load, `mikan_step` runs once per fixed step.
A script that counts its own calls, or reads `pos_x`, is doing exactly what
the contract allows: the arguments are the whole world it sees.

## What the engine guarantees about the return values

The engine, not the script, decides what is valid:

- `dx`, `dy`, and `speed` must be finite when converted to `f32` (the
  simulation's precision). A value that overflows `f32` is a bug, not a clip.
- `speed` must be non-negative.
- A direction **longer than unit length is normalized** (so `return 3, 4, 60`
  moves at 60 px/s along `(0.6, 0.8)`); a shorter one is left alone, so a
  script can intentionally walk at a fraction of its speed.

Nothing non-finite or negative can enter the simulation.

## The sandbox

Scripts are treated as untrusted input even when their authors are trusted.
The VM is built with an **explicit safe library set** — coroutine, table,
string, math, utf8, bit32, and Luau's side-effect-free `os` (clock, date,
difftime, time only) — and **no** I/O, package or module loading, `debug`, or
FFI. On top of that, `lua.sandbox(true)` enables Luau's own sandbox: the
global environment is read-only and chunk writes go through a local proxy. A
**64 MiB memory limit** turns a runaway script into a Lua error instead of an
out-of-memory kill.

Scripts can compute anything; they cannot read or write files, reach the
network, start processes, or load native code. The memory limit bounds memory,
not time — a long `mikan_step` still costs the frame it runs in (instruction
quotas are future work, stated rather than implied).

## Failure semantics

Two severities, both with tests:

- **Load time is fatal.** An unreadable file, a compile error, or a missing
  global `mikan_step` fails `from_file`/`from_source` with a `ScriptError`
  naming the script and the line (`player.luau:2: ...`). The demo reports the
  error and exits — a script that cannot honor the contract must not half-run.
- **Run time is loud and survivable.** An error raised inside `mikan_step`, a
  wrong return count or type, a non-finite number, or a negative speed fails
  that one `call_step` with `Runtime` or `InvalidReturn`. The engine has no
  opinion about what happens next; the demo logs the first error with its
  script, line, and message, then idles for the step and keeps running.

## The demo's integration

`--script PATH` selects the gameplay script (default
`games/demo2d/scripts/player.luau`). The demo's `update` reads the held keys,
the fixed step, and the player's position; calls `call_step`; and writes the
result into the `Controls` resource before the schedule runs. The systems are
unchanged: `input` copies `Controls` into `Motion`, `movement` integrates,
`collide` resolves, `clamp` bounds.

The default script *is* the classic player's movement code:

```lua
local SPEED = 420 -- pixels per second

function mikan_step(dt, left, right, up, down, pos_x, pos_y)
    local x = (right and 1 or 0) - (left and 1 or 0)
    local y = (down and 1 or 0) - (up and 1 or 0)
    if x == 0 and y == 0 then
        return 0, 0, 0
    end
    local length = math.sqrt(x * x + y * y)
    return x / length, y / length, SPEED
end
```

Eight directions, diagonals not faster, 420 px/s — the same behavior the Rust
implementation had, now written in Luau where designers can change it without
rebuilding the engine.

## Verification

- **Unit tests** (`mikan-script`): the contract and every failure path — a
  valid step; step after step with changing arguments; short directions kept;
  over-long directions normalized; non-finite values, negative speed, wrong
  return counts, and wrong return types rejected as `InvalidReturn`; runtime
  errors carrying script and line; missing files; a missing or non-function
  `mikan_step`; the sandbox hiding I/O, `debug`, module loading, and process
  control; and the memory limit stopping runaway allocation.
- **`scripts/verify_script.sh`** (Xvfb, end to end): the default script
  reproduces the documented 3.5 px per moved step; a script returning
  `(3, 4, 60)` for exactly 60 steps ends at `[498, 324]` — the normalized
  `(0.6, 0.8)` path, distinguishable from the unnormalized `[570, 420]`; a
  syntax error, a missing `mikan_step`, and a missing file each fail startup
  with exit 1 and a clear message; a runtime error and a non-finite return are
  each logged exactly once, move nothing, and leave the game running.

## Limits, stated plainly

- Single-threaded, one script per engine, no hot reload; scripts load at
  startup.
- Lua numbers cross the boundary as `f64` and are validated in `f32`; the
  determinism contract in `docs/ecs.md` applies to gameplay, not to a
  fixed-point simulation.
- The default script path is repository-relative, like `--assets`; pass
  `--script` when running from elsewhere.
- The Luau version is pinned through `mlua` (`0.11.6`: the newest whose Rust
  requirement, 1.85, stays under the workspace MSRV 1.87) and upgraded by
  deliberate bump.
