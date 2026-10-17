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

echo "==> generator check: the committed assets match scripts/generate_assets.py"
python3 scripts/generate_assets.py --check

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

        echo "==> action input verification: C backend and C++ runtime under Xvfb"
        RUST_LOG="${RUST_LOG:-info}" xvfb-run -a bash scripts/verify_input_actions.sh

        echo "==> camera verification: follow camera and zoom under Xvfb"
        RUST_LOG="${RUST_LOG:-info}" xvfb-run -a bash scripts/verify_camera.sh

        echo "==> ecs verification: player entity, scheduled systems, fixed steps"
        RUST_LOG="${RUST_LOG:-info}" xvfb-run -a bash scripts/verify_ecs.sh

        echo "==> collision verification: blocks, collide system, diagonal resolution"
        RUST_LOG="${RUST_LOG:-info}" xvfb-run -a bash scripts/verify_collision.sh

        echo "==> scripting verification: Luau drives movement; failure paths behave"
        RUST_LOG="${RUST_LOG:-info}" xvfb-run -a bash scripts/verify_script.sh

        echo "==> scene verification: transform hierarchy, rigid spin, deferred despawn"
        RUST_LOG="${RUST_LOG:-info}" xvfb-run -a bash scripts/verify_scene.sh
    else
        echo "==> skipping the input, camera, ecs, collision, scripting, and scene verification: xdotool is not installed"
    fi

    echo "==> showcase verification: rotation, tint, alpha, samplers, PNG alpha"
    RUST_LOG="${RUST_LOG:-info}" xvfb-run -a bash scripts/verify_showcase.sh

    echo "==> stress verification: 1000 sprites in one batch and one draw call"
    RUST_LOG="${RUST_LOG:-info}" xvfb-run -a bash scripts/verify_stress.sh

    echo "==> pixel-perfect verification: integer zoom and physical-grid alignment"
    RUST_LOG="${RUST_LOG:-info}" xvfb-run -a bash scripts/verify_pixel_perfect.sh

    echo "==> asset verification: generator check and readable failure modes"
    RUST_LOG="${RUST_LOG:-info}" xvfb-run -a bash scripts/verify_assets.sh
else
    echo "==> skipping the headless and runtime verification: xvfb-run is not installed"
fi

echo
echo "all checks passed"
