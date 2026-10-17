# 0011. Gameplay scripting with embedded Luau

- Status: Accepted
- Date: 2026-10-16

## Context

The roadmap's Phase 5 is 2D collision (ADR 0010). During the phase, the project
owner directed a second deliverable into Phase 5: **gameplay scripting must
become real now**, in a form consistent with Mikan's stated architecture — Rust
and C++ forming the engine core, C as the low-level foundation. This record is
that directive's ADR, and together with ADR 0010 it is also the roadmap
amendment: Phase 5 is collision *plus* scripting, by owner decision, stated
here rather than silently absorbed.

The project-owner directive from the collision ADR ("the three-language
architecture is a requirement, not a long-term intention") names C++ as a core
language. Collision gave C its first real home (ADR 0010); C++ needs a
substantive one. A scripting VM is the honest fit: game engines embed scripting
layers precisely because gameplay decisions — which direction to move, how
fast, what to do on collision — are iterated on by people who are not engine
programmers, and that iteration should not require a full Rust rebuild.

Requirements carried into this decision:

1. **Real scripting, not a stub.** A general-purpose embedded language that
   authors can actually write gameplay in, with a documented step contract,
   error reporting with file and line information, and verified failure paths.
2. **Sandboxed by construction.** Scripts must not reach the filesystem,
   network, process, or native code, and a runaway script must not take the
   process down. The engine treats scripts as untrusted input even when the
   authors are trusted.
3. **Deterministic integration.** The script runs before the fixed-step
   schedule and only through a narrow data contract; it cannot touch the ECS,
   the renderer, or the clock.
4. **C++ must be genuine.** If C++ is in the architecture, it must do real
   work here, and the record must say exactly what work that is.

Open questions this record answers:

1. **Which language and VM?** Evaluated: Lua 5.4 (portable, but slower and no
   built-in sandboxing), LuaJIT (a JIT — wrong portability and sandboxing
   profile), Rhai (pure Rust — would make the C++ core claim decorative in the
   scripting layer, and it is a far less standard language for game authors),
   WebAssembly (a heavier boundary and a second toolchain for content authors
   to learn), and a custom DSL (invents a language; excluded: "real" is a
   requirement). **Luau** — the statically-optimized Lua derivative used in
   Roblox production — wins on interpreter speed, a first-class sandboxing
   model (`luaL_sandbox`), a small embedding surface, vendored-source builds,
   and the MIT license.
2. **Which binding?** `mlua`, the maintained Rust binding, with the `luau` and
   `vendored` features so the workspace builds the pinned Luau sources with
   the system C++ compiler and depends on no system-only Lua installation.
   Version `0.11.6`: its Rust requirement (1.85) stays under the workspace's
   MSRV 1.87; `0.12` requires 1.88 and is deliberately not adopted yet.
3. **Where is the boundary?** Scripts own *intent only*: one global step
   function in, three numbers out. The engine owns everything else —
   scheduling, movement integration, collision, clamping, resources. The
   script cannot see or mutate world state; position and the held keys are
   passed in as arguments, like a pure function.
4. **What is the step contract?**

   ```lua
   function mikan_step(dt, left, right, up, down, pos_x, pos_y)
       return dx, dy, speed  -- a direction (any length) and pixels/second
   end
   ```

   `dt` is the fixed step in seconds; the four booleans are the held keys;
   `pos_x, pos_y` is the player's current position in world pixels. The return
   is three numbers. A missing function, a wrong return type, a non-finite
   number, a negative speed, or a runtime error are all defined failure paths
   with tests, not crashes.
5. **What happens on failure?** Two severities:
   - **Load time** (missing file, syntax error, no `mikan_step`): fatal. The
     application reports the error — with file name and line where the Luau
     compiler provides it — and exits. A script that cannot honor the contract
     must not half-run.
   - **Run time** (error raised inside `mikan_step`, or an invalid return):
     the step falls back to no input, the error is logged **once** per run
     with its message and line, and the game keeps running. A gameplay bug
     must not kill the host mid-frame; it must be loud and non-fatal.

## Decision

1. **New crate `crates/script`** — safe Rust only (the workspace's one
   `unsafe` crate remains `mikan-collision-sys`). It exposes:
   - `ScriptEngine::from_file(path)` and `ScriptEngine::from_source(name,
     source)`;
   - `ScriptEngine::call_step(StepInput) -> Result<StepOutput, ScriptError>`
     with `StepInput { dt, left, right, up, down, position }` and
     `StepOutput { direction, speed }`;
   - `ScriptError` with the variants `Load`, `Compile`, `MissingFunction`,
     `Runtime`, `InvalidReturn`, and `Vm` (VM setup), each carrying the
     information needed to point at the cause.

2. **The engine validates; the script is never trusted.** `dx`, `dy`, and
   `speed` must be finite; `speed` must be non-negative; a direction longer
   than unit length is normalized (shorter vectors are left alone so a script
   can intentionally walk slowly, at a fraction of `speed`). Nothing
   non-finite or negative can enter the simulation.

3. **Sandboxed VM.** Only an explicit safe library set is loaded — coroutine,
   table, string, math, utf8, bit32, and Luau's side-effect-free `os`
   (clock, date, difftime, time): no I/O, no package or module loading, no
   `debug`, no FFI. On top of that, `lua.sandbox(true)` enables Luau's
   sandbox (read-only global environment with a local write proxy); and a
   64 MiB memory limit turns runaway allocation into a Lua error. Scripts
   are loaded as chunks with their file name as the chunk name, which is
   what makes runtime errors report `player.luau:LINE:`.

4. **C++'s role is stated exactly.** The C++ in this phase is the **vendored
   Luau VM**: several hundred kilobytes of upstream C++ sources compiled into
   the binary by `mlua-sys`'s build script. Mikan writes **no hand-written
   C++ glue** in this milestone, and this record and the docs say so instead
   of implying otherwise. The Luau sources are third-party: the
   warnings-as-errors rule of `docs/ffi.md` applies to Mikan's own native
   sources (the C kernel, ADR 0010), not to pinned upstream code compiled by
   its own build system.

5. **Demo integration.** The demo gains `--script PATH` (default
   `games/demo2d/scripts/player.luau`). The classic player's key handling and
   speed constant move into that script: the Rust side reads the held keys,
   passes them in, and writes the returned direction and speed into a
   `Controls` resource before the schedule runs; `input`, `movement`,
   `collide`, and `clamp` remain engine systems. The default script reproduces
   the exact behavior the Rust implementation had (diagonal-normalized,
   420 px/s), which keeps every existing verification script green and proves
   the script path is real — the game's movement now *is* the script.

6. **Verification is end to end, failure paths included.**
   `scripts/verify_script.sh` checks: the default script drives the documented
   movement; a syntax error fails startup with file and line; a missing
   `mikan_step` fails startup; a runtime error logs once and leaves the game
   responsive (no movement, no crash); a NaN return is rejected as
   `InvalidReturn`; and an over-long direction is normalized (a script
   returning `(3, 4, 60)` traces the documented 4:3 path). `mikan-script`'s
   own tests cover the same contract at the unit level, including the sandbox
   (`io`, `debug`, and module loading do not exist; Luau's safe `os` does).

## Consequences

Positive:

- Gameplay decisions are scriptable for real, in a sandboxed, pinned,
  reproducible VM, through a seven-argument, three-return boundary — the
  smallest scripting surface that supports the milestone.
- C++ now does substantive work in the engine (the VM), and the record is
  honest that no hand-written C++ glue exists yet.
- Failure behavior is defined on purpose: fatal at load, loud-and-survivable
  at run time, with line numbers in both.
- MSRV remains 1.87; the Luau version is pinned through `mlua` and upgraded by
  deliberate bump, not drift.

Negative:

- **A C++ compiler is now a build prerequisite** in addition to C. The
  repository previously said no C/C++ toolchain was needed; `DEVELOPMENT.md`,
  `ffi.md`, and CI must say otherwise now (this record is the authority for
  that change).
- Build time grows: the Luau sources compile once per target. That cost is
  paid on every clean build and is reported, not hidden.
- The scripting contract is `f64`-mediated (Lua numbers are doubles);
  the demo converts to `f32` at the boundary. Determinism across platforms is
  only as stable as double-precision Lua arithmetic plus the engine's
  validation — sufficient for gameplay, not a substitute for a fixed-point
  simulation.
- Scripts are single-threaded and can still waste CPU in a long `mikan_step`
  (the memory limit bounds memory, not time). Instruction-count quotas are
  future work; the docs say this rather than implying full resource control.
- One more language to read when debugging the demo, and the default script
  path is repo-relative, like `--assets`.
