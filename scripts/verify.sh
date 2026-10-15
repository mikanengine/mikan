#!/usr/bin/env bash
#
# Runs every quality gate CI enforces, plus the headless smoke test when a
# virtual display is available. One command, no arguments:
#
#     ./scripts/verify.sh
#
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> cargo fmt --all -- --check"
cargo fmt --all -- --check

echo "==> cargo check --workspace --all-targets"
cargo check --workspace --all-targets

echo "==> cargo test --workspace"
cargo test --workspace

echo "==> cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> cargo build --workspace --release"
cargo build --workspace --release

if command -v xvfb-run >/dev/null 2>&1; then
    echo "==> headless smoke test: 120 frames under Xvfb, captured and verified"
    mkdir -p artifacts
    RUST_LOG="${RUST_LOG:-info}" xvfb-run -a ./target/release/demo2d \
        --frames 120 \
        --capture artifacts/frame.ppm \
        --stats
    python3 scripts/verify_capture.py artifacts/frame.ppm

    if command -v xdotool >/dev/null 2>&1; then
        echo "==> input verification: synthetic key events under Xvfb"
        RUST_LOG="${RUST_LOG:-info}" xvfb-run -a bash scripts/verify_input.sh

        echo "==> camera verification: follow camera and zoom under Xvfb"
        RUST_LOG="${RUST_LOG:-info}" xvfb-run -a bash scripts/verify_camera.sh
    else
        echo "==> skipping the input and camera verification: xdotool is not installed"
    fi
else
    echo "==> skipping the headless smoke test: xvfb-run is not installed"
fi

echo
echo "all checks passed"
