# 0001. Rust as the primary implementation language

- Status: Accepted
- Date: 2026-10-15

## Context

Mikan needs a primary language for the engine core: memory management, a
scheduling loop, graphics and platform integration, and a modular codebase that
can absorb C and C++ later where they are genuinely justified.

The realistic candidates were:

- **C++** — maximum ecosystem and tooling familiarity in game development; manual
  lifetime management; large surface for memory-safety defects; heavy template
  and build-system complexity.
- **C** — minimal, predictable, ubiquitous; no generics or RAII; every resource
  hand-managed; error handling by convention.
- **Zig / Odin** — smaller ecosystems, less mature tooling for graphics and
  windowing, more risk for a long-lived project.
- **Rust** — memory safety without a garbage collector, first-class C FFI,
  built-in test/format/lint tooling, and a mature, actively developed graphics
  stack (`wgpu`) and windowing stack (`winit`).

Game engines have historically been written in C++ because it was the only
practical choice, not because it was the best fit for correctness-critical,
concurrent systems. The failure modes that hurt engines most — use-after-free,
data races, aliasing bugs that appear only under load — are exactly the class of
defect Rust rejects at compile time.

## Decision

Rust is the primary implementation language for Mikan. The engine crates, the
application layer, and the tooling are Rust. New subsystems start in Rust, and a
non-Rust implementation is an exception that requires an accepted ADR with a
measured justification (see ADR 0004).

Supporting practices:

- The workspace forbids `unsafe_code` in first-party code. Third-party crates
  carry their own `unsafe`; that is their business, and it is why the dependency
  graph is kept small.
- The toolchain is pinned in `rust-toolchain.toml`; formatting, linting, and
  testing are mandatory gates in CI, so the language's guarantees are not eroded
  by inconsistent configuration.
- The lockfile is committed. An engine's dependency resolution is part of its
  build contract.

## Consequences

Positive:

- Whole classes of defects (use-after-free, double free, data races on shared
  state, iterator invalidation) are compile errors rather than crashes under load.
- One toolchain covers build, test, format, lint, documentation, and benchmarks,
  which keeps the development loop fast and the CI configuration small.
- The C ABI is a first-class concern of the language, so native modules can be
  added later without restructuring the engine (see ADR 0004).
- Error handling is explicit and typed, which is what allows this engine to treat
  recoverable graphics conditions as values instead of exceptions.

Negative:

- Some kinds of prototype-speed iteration are slower than in a scripted or
  dynamic language, and certain C++ template-heavy libraries are unavailable
  without a wrapper.
- Compile times grow with generic-heavy dependencies; they are kept in check by
  limiting the dependency graph rather than by avoiding generics in our own code.
- Engineers joining from C++ engines have to learn Rust's ownership model, which
  is a real onboarding cost.
