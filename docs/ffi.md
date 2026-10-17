# Native code (FFI) policy

Mikan is written primarily in Rust. This document defines when native C or C++
code is allowed to enter the repository, how it must be structured, and what
"done" means for such a module. It also states, unambiguously, what native code
exists today.

## Current status: three native modules

Native code entered the repository by explicit owner-directed architecture
decision (ADR 0010, ADR 0011, ADR 0012) rather than silently:

1. **`crates/collision-sys` (C99)** — the collision narrow-phase kernel:
   one batch function, `mikan_contacts`, over plain `#[repr(C)]` data, with a
   safe wrapper (`crates/collision`), warnings-as-errors on its own
   sources, cross-check tests against an independent Rust implementation, and
   a recorded Valgrind run (`docs/STATUS.md`). It was **not** adopted on
   performance grounds: it measures at parity with the Rust reference and no
   speedup is claimed. ADR 0010 supersedes ADR 0004 *for this module only*.
2. **`crates/scene-sys` (C and C++)** — the scene module: a C compose kernel
   (`mikan_transform2d_compose`, one batch-shaped function over a seven-`f32`
   `#[repr(C)]` struct) next to a hand-written C++ scene graph
   (`mikan_scene_graph`, one opaque handle with `extern "C"` entry points and
   status codes), with a safe wrapper (`crates/scene`), warnings-as-errors and
   `-fno-exceptions -fno-rtti` on its own sources, cross-check tests against an
   independent Rust compose, and a recorded Valgrind run (`docs/STATUS.md`).
   Like collision it was **not** adopted on performance grounds: the native
   graph measures *slower* than a Rust array fold because it carries real graph
   bookkeeping, and no speedup is claimed. It is the first module with
   hand-written C++ (not just a vendored C++ library). ADR 0012 supersedes
   ADR 0004 *for this module only*.
3. **The vendored Luau VM (C++)** — `mlua`'s `luau` + `vendored` features
   compile the pinned upstream Luau sources with the system C++ compiler when
   `mikan-script` is built (ADR 0011). It is third-party code consumed
   through `mlua`; Mikan writes no C++ glue for it, and the rules below apply
   to Mikan's own sources, not to upstream code compiled by its own build
   system.

Consequently, **a C compiler and a C++ compiler are build prerequisites** on
every platform, documented in `docs/DEVELOPMENT.md`. The workspace still
forbids `unsafe_code` crate-wide; the exceptions are `mikan-collision-sys` and
`mikan-scene-sys`, whose crate roots record the overrides and their reasons.

The first-party C surface is two batch kernels (the collision narrow phase and
the transform compose); the hand-written C++ surface is the scene graph; and the
third-party C++ surface is the vendored Luau VM. Nothing else links native code.

## Why C and C++ were deferred, and what changed

Using a second and third language has real costs that must be paid repeatedly,
not once:

- every build needs a C/C++ toolchain, correct flags, and reproducible versions;
- every boundary is an opportunity for a memory-safety bug that Rust would have
  caught at compile time;
- errors, ownership, and thread-safety rules must be re-expressed and re-tested
  across the boundary;
- debugging spans two debuggers and two sets of symbols;
- cross-compilation and platform packaging get harder.

A module earns its place only when it does something Rust cannot do at
acceptable cost. "It is faster in C++" is not sufficient on its own — the
measurement must be shown, and the same measurement must be repeated after the
module changes. ADR 0004 records this decision and the criteria below.

What changed in Phase 5: the project owner directed that Mikan's stated
architecture — Rust + C++ forming the engine core, C as the low-level
foundation — is a requirement, not a long-term intention. ADR 0010 (the C
collision kernel) and ADR 0011 (the vendored C++ Luau VM) record how that
directive is honored; the scene milestone added ADR 0012 (a C transform kernel
and Mikan's own C++ scene graph). The policy below remains in force for every
module proposed **on performance grounds**; a module introduced by an
architecture decision instead must say so in its own ADR, report its
measurements as measured (parity — or being slower — is a legitimate result,
stated as such), and meet every other criterion here unchanged.

## Criteria for introducing a native module

A native module is considered only when **all** of the following hold:

1. **A concrete need exists.** A profiled bottleneck, a required third-party
   library without a safe Rust equivalent, or a documented external interface
   obligation. No speculative modules, and no "foundation" directories that
   compile nothing.
2. **The boundary is narrow.** A handful of functions with plain data types
   (integers, floats, pointers plus lengths, opaque handles). No C++ classes,
   exceptions, templates, or STL types crossing the boundary.
3. **It is measured.** The Rust implementation exists first and its performance
   is recorded; the native replacement must beat it by a margin that justifies
   the costs above, using the benchmark protocol in `DEVELOPMENT.md`. This
   criterion binds every module proposed on performance grounds; the only way
   past it is an explicit ADR stating that the driver is architectural, what
   the honest measured result is, and that no speedup is claimed — ADR 0010 is
   the existing example.
4. **It is isolated.** The module lives in its own crate (for example
   `crates/<name>-sys`) that nothing imports directly except a safe wrapper.
5. **It is tested on its own.** The module has tests exercising the C/C++ code
   through the wrapper, including error paths and boundary conditions, and those
   tests run in CI on every supported platform.

## Required structure of a native module

- **`-sys` crate owns the native code.** The C/C++ sources, the `build.rs` that
  compiles them, and the raw `extern "C"` declarations live in one small crate.
  Nothing else links the native library.
- **A safe wrapper crate is the only public face.** Callers never see raw
  pointers, lifetimes, or `unsafe`. The wrapper exposes Rust types with Rust
  semantics: `Result` for errors, `Drop` for release, `Send`/`Sync` only when the
  native code genuinely guarantees it.
- **`unsafe` is confined.** `-sys` crates may use `unsafe`; every occurrence has
  a `// SAFETY:` comment stating the invariant that makes it sound. The workspace
  lint that forbids `unsafe` is overridden explicitly, per crate, with the reason
  recorded in the crate root — never silently at the item level.
- **The ABI is `extern "C"`.** For C++ implementations, only `extern "C"`
  functions may be exported; wrappers around C++ objects are compiled in C++ and
  expose opaque handles.
- **Errors translate at the boundary.** The native side returns status codes or
  error structs; the wrapper converts them into Rust error types. No panics
  unwind across the boundary, and no Rust panics are caught and ignored.
- **Ownership is explicit.** Every allocation has exactly one documented owner
  and exactly one function that frees it. The wrapper enforces this with types,
  not with documentation alone.
- **Thread safety is documented and enforced.** Whether a function may be called
  concurrently, from which threads, and whether the runtime state is immutable
  after initialization is stated in the C/C++ header and mirrored in the Rust
  wrapper's type bounds.

## C vs C++

- **C** is the default for new native code: smaller ABI surface, no name
  mangling complications, no runtime initializers, and easier static integration.
  Suitable for allocators, math kernels, SIMD, and small platform shims.
- **C++** requires a specific reason: an existing library that must be consumed
  on its own terms, or a workload where C++ abstractions measurably matter. Even
  then, only `extern "C"` entry points may cross the boundary; the C++ layer must
  not own control flow that Rust depends on. The vendored Luau VM is the
  "existing library consumed on its own terms" case: Mikan does not write the
  C++, it compiles pinned upstream sources through `mlua` and never links
  against an ambient Lua installation. The scene graph in
  `crates/scene-sys/src/scene_graph.cpp` is the other case: a dynamic
  parent/children topology is a mutually-referential data structure whose
  invariants (forest, no cycles, stable order) are natural in C++ and awkward
  to express across a boundary, so it is written in C++ behind one opaque
  handle — while the arithmetic it calls is C.
- **Both** must build with warnings-as-errors for the module's own sources and
  must not require exceptions or RTTI for C++. Vendored third-party sources are
  compiled by their own build system under their own flags; the rule binds the
  code Mikan owns.

## What a native module must never do

- Own the frame loop, the window, or the GPU device. Those belong to the Rust
  layers that already handle their error and lifecycle semantics.
- Allocate on a per-frame path without a documented reason and a profiling
  result.
- Cross the boundary per object per frame. Vertices, matrices, and instance data
  cross as slices, not element by element.
- Introduce a second threading model. A native module runs on the thread that
  Rust calls it from unless its header documents otherwise and the wrapper
  encodes that in its `Send`/`Sync` bounds.

## Verification for a native module

A native module is "done" only when the following are all true:

- `cargo test --workspace` exercises the module through its safe wrapper on
  Linux, Windows, and macOS in CI;
- the module's own C/C++ sources pass their configured warnings-as-errors build;
- the wrapper has a doc test or an integration test demonstrating correct use,
  including at least one error path;
- sanitizer or Valgrind coverage has been run at least once for the module, with
  the command and result recorded in `docs/STATUS.md`;
- `docs/ARCHITECTURE.md` describes where the module sits in the dependency graph.

A module that cannot show all of this does not belong in this repository. Two
first-party modules have walked the whole path and their records are the worked
examples: collision — design in `docs/collision.md`, decision in ADR 0010,
benchmark and Valgrind results in `docs/STATUS.md` — and the scene module —
design in `docs/scene.md`, decision in ADR 0012, benchmark and Valgrind results
in `docs/STATUS.md`.
