# 0005. Cross-platform validation distinguishes compile-verified from runtime-verified

- Status: Accepted
- Date: 2026-10-15

## Context

The engine targets Windows, Linux (X11 and Wayland), and macOS, and the code
avoids platform-specific branches entirely: `winit` and `wgpu` own the platform
differences, and first-party code is portable Rust.

That portability claim is easy to make and hard to substantiate. A crate that
"should work everywhere" is not evidence that it does. The temptation is to
describe cross-platform support in terms of intent — "supports Windows, Linux,
and macOS" — which then quietly becomes a claim that the project has verified
something it never ran.

Realistically, a development machine or CI container can build for other
platforms easily (`cargo check`, cross-compilation) but cannot execute them
without real hardware or virtualized environments. These two levels of assurance
are fundamentally different and must not be conflated.

## Decision

Every platform claim in this repository carries an explicit verification level:

- **Compile-verified** — `cargo check --workspace --all-targets` succeeds for
  that target triple, on that platform's CI runner. This proves the code and the
  dependency graph build; it proves nothing about runtime behavior.
- **Runtime-verified** — the demo was executed on that platform, presented a
  target number of frames, handled its window lifecycle, and (where capture is
  supported) produced a captured frame that passed `scripts/verify_capture.py`.

Rules that follow:

1. **CI compiles all three platforms on every change.** The
   `cross-platform-check` job runs `cargo check --workspace --all-targets` on
   `windows-latest` and `macos-latest`; the `quality` job does the same on
   Linux, plus tests, lint, and a release build.
2. **Runtime verification happens where a display and a graphics device are
   available.** In CI, the `smoke` job runs the demo under `Xvfb` with Mesa's
   `lavapipe` software Vulkan device, presents a fixed 120 frames, and verifies
   the captured frame. That is a real runtime verification of Linux/X11 on a
   software device — strong evidence about lifecycle and correctness, no evidence
   about GPU performance.
3. **Only what was actually run is called verified.** `docs/STATUS.md` and the
   README state the verification level per platform, with the date and the exact
   command. Windows and macOS are compile-verified until someone runs them and
   records the result.
4. **Hardware-specific claims require hardware results.** Frame rates, GPU
   compatibility, and driver-specific behavior are only reported from a run on
   that hardware, with the environment recorded. Software-rasterizer numbers are
   labeled as such.
5. **Verification regressions are bugs.** If a platform that was
   runtime-verified stops working, that is treated with the same severity as a
   broken test, including a note in `docs/STATUS.md` until it is restored.

## Consequences

Positive:

- The project's platform claims are auditable: each one traces to a command and
  a date.
- CI catches portability regressions (a `#[cfg]` mistake, a Linux-only API) on
  every change, without pretending to have run them.
- Headless runtime verification is available to every contributor and to CI at
  negligible cost, so the demo cannot silently stop working.
- Readers of the documentation can tell the difference between "this builds" and
  "this runs".

Negative:

- The documentation must be updated when verification status changes, and it must
  resist rounding up compile-verified to verified.
- CI needs a somewhat unusual setup (`Xvfb` plus `mesa-vulkan-drivers`) to run
  the demo, and that job is slower than a plain build.
- Real macOS and Windows runtime verification remains outstanding work until
  someone with those machines runs it, so the roadmap carries an explicit item
  for it rather than assuming it is fine.
