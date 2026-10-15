# 0004. C and C++ modules are deferred until justified by a measured need

- Status: Accepted
- Date: 2026-10-15

## Context

Game engines commonly combine a high-level language for gameplay with C or C++
for hot paths. Mikan's stated direction is Rust at the core, C at the foundation,
and C++ where it counts — but that phrasing describes an intention, not a plan,
and it invites the worst possible outcome: empty `foundation/` or `cpp/`
directories, build scripts that compile nothing, and interfaces designed before
anything concrete exists to justify them.

Multi-language builds impose costs that are paid on every commit, not once:

- a C/C++ toolchain becomes a prerequisite for every contributor and CI job;
- the boundary is a place where Rust's memory and thread-safety guarantees stop,
  so each crossing needs its own invariants and tests;
- errors, ownership, and lifetime rules must be expressed twice, in two type
  systems, and kept in sync;
- cross-compilation, packaging, and debugging become two-language problems;
- an abstraction built before its use case is usually wrong.

Rust, meanwhile, compiles to the same machine code and links against the same
libraries as C. For most engine subsystems, "C would be faster" is a hypothesis,
not a measurement.

## Decision

No C or C++ module is introduced into Mikan until a measured need justifies it.
Concretely:

1. **Nothing native is scaffolded speculatively.** No placeholder crates, no
   `build.rs` compiling an empty source file, no FFI surface for code that does
   not exist. The repository contains no C or C++ today, and that is the correct
   state until the criteria below are met.
2. **The Rust implementation comes first, with measurements.** Every subsystem
   starts in Rust. If profiling shows it is a bottleneck, the measurement is
   recorded (hardware, dataset, compiler flags, methodology) before a rewrite is
   considered.
3. **A native module must clear all five criteria** in `docs/ffi.md`: a concrete
   need, a narrow boundary, a measured advantage over the Rust implementation,
   isolation in its own `-sys` crate, and its own tests that run in CI.
4. **When it does arrive, it must follow the required structure** in
   `docs/ffi.md` — a `-sys` crate owning the raw code, a safe wrapper as the only
   public face, `unsafe` confined and documented, `extern "C"` ABI, errors
   translated at the boundary, explicit ownership and thread-safety contracts.
5. **The decision to add one is itself an ADR.** Superseding this record requires
   stating the need, the measurement, the boundary design, and the verification
   plan — not merely a preference.

This decision constrains the shape of the repository, not just its contents:
nothing may be built that assumes native modules exist (no macro that generates
`extern "C"` scaffolding, no build step that expects a C compiler).

## Consequences

Positive:

- The build has one toolchain, so setup, CI, and cross-compilation stay simple.
- All first-party code is subject to the workspace's `unsafe_code = "forbid"`
  lint; there is no unverified memory management in the repository.
- Native modules, when they arrive, will arrive with a measured reason and a
  design shaped by an actual use case instead of by speculation.
- Contributors cannot accidentally create the appearance of native support that
  does not exist.

Negative:

- A genuinely faster C implementation of some future subsystem will not be
  written until the Rust version is measured against it, which is slower than
  writing C first.
- If a required third-party library only exists in C or C++, the work to bring
  it in is larger, because it must arrive with a wrapper and tests.
- The engine may carry a Rust implementation that is slower than an obvious C
  equivalent for a while, in the name of avoiding premature complexity.
