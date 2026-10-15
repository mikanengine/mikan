# Contributing to Mikan

Thanks for taking the time to contribute. This document describes how to get a
change from an idea to a merged commit.

## Before you start

- Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for how the crates fit
  together and [`docs/adr/`](docs/adr/) for the decisions that shape the code.
- For anything larger than a bug fix, open an issue first. Mikan is
  intentionally small, and discussing scope up front avoids wasted work.
- Decisions that change the architecture (a new dependency with a large surface,
  a new layer, native code, a rendering strategy) need an ADR, added as the next
  number in `docs/adr/`. Write it as part of the change.

## Development setup

See [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) for prerequisites, build, run,
and headless verification instructions.

## The quality gates

Every change must pass, locally and in CI:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --release
```

Changes that affect windowing, input, or rendering should also be exercised with
the demo, including the headless smoke test where a display is unavailable:

```bash
xvfb-run -a cargo run -p demo2d --release -- --frames 120 --capture artifacts/frame.ppm --stats
python3 scripts/verify_capture.py artifacts/frame.ppm
```

## Code guidelines

- **Match the surrounding code.** Naming, module layout, error style, and
  documentation tone are consistent across crates; keep them that way.
- **Comment the why, not the what.** Explain invariants, trade-offs, and
  decisions that are not obvious from the code. Do not narrate the code.
- **Every public item has a doc comment.** Workspace lints enforce this, and CI
  treats warnings as errors.
- **No `unsafe` in first-party code.** The workspace forbids it. If a change
  needs it, the change needs an accepted ADR and a confined, documented boundary
  as described in [`docs/ffi.md`](docs/ffi.md).
- **Tests live next to the code they test**, in `#[cfg(test)] mod tests`. Pure
  logic must be unit-testable without a window or a GPU; if your change is hard
  to test, that is feedback about its structure.
- **Errors are typed and explicit.** Recoverable conditions are values, not
  panics or silent warnings. `unwrap`/`expect` are for tests and for states that
  are genuinely impossible; document why when you use them in library code.
- **No performance claims without measurements.** If you claim a change is
  faster, include the before and after numbers, the hardware, and how they were
  produced. Otherwise state that it is unmeasured.
- **New dependencies need a reason.** Mikan's value includes what it does not
  pull in. Additions are discussed in the pull request, and versions are pinned
  in the workspace manifest.

## Commits and pull requests

- Keep commits focused: one logical change per commit, building and passing tests
  on its own.
- Write commit messages in the imperative mood with a short summary line, for
  example `render: reconfigure swap chain after suboptimal acquire`. A body is
  welcome when the reason for the change is not obvious.
- In the pull request description, state what changed, why, and how it was
  verified. Include the exact commands you ran. If something is untested, say so
  plainly — an honest gap is far more useful than an implied guarantee.
- Update documentation when behavior, commands, or interfaces change, and add a
  `CHANGELOG.md` entry under `Unreleased`.

## Reporting bugs

Include the platform (OS, window system, GPU and driver), the exact command and
output (run with `RUST_LOG=debug` for engine diagnostics), and whether the
problem reproduces in the headless smoke test. A capture (`--capture`) helps for
anything visual.

## License

Mikan is dual licensed under MIT or Apache-2.0, at your option. Unless you state
otherwise, any contribution you intentionally submit for inclusion in the work
is dual licensed as above, without additional terms or conditions.
