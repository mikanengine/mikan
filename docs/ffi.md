# Native code (FFI) policy

Mikan is written primarily in Rust. This document defines when native C or C++
code is allowed to enter the repository, how it must be structured, and what
"done" means for such a module. It also states, unambiguously, what native code
exists today.

## Current status: none

There are **no C or C++ modules, no `build.rs` steps that invoke a C compiler,
no `-sys` crates, and no `extern "C"` declarations** in this repository. The only
`unsafe` in the dependency graph belongs to third-party crates (`wgpu`, `winit`,
and their dependencies); the workspace forbids `unsafe_code` crate-wide, so first
party code cannot add any without changing a lint.

The C and C++ toolchain is therefore not required to build, test, or run Mikan
today. The `crates/` directory contains only Rust.

## Why C and C++ are deferred

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
   the costs above, using the benchmark protocol in `DEVELOPMENT.md`.
4. **It is isolated.** The module lives in its own crate (for example
   `crates/mikan-<name>-sys`) that nothing imports directly except a safe wrapper.
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
  not own control flow that Rust depends on.
- **Both** must build with warnings-as-errors for the module's own sources and
  must not require exceptions or RTTI for C++.

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

Until all of that holds, the module does not exist in this repository.
