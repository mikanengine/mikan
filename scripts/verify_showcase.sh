#!/usr/bin/env bash
#
# Verifies the showcase scene end to end: rotation, tinting, alpha, per-sprite
# samplers, and PNG alpha, each at a fixed position in a captured frame.
#
# Checks:
#   1. The demo log reports the assets that were loaded and the render stats:
#      8 sprites in 4 batches and 4 draw calls (one per texture/sampler pair,
#      so the adjacent tiles really batch together).
#   2. The captured frame matches the scene's documented pixels; the checks
#      live in scripts/verify_showcase.py.
#
# Requires: a display (Xvfb is fine), python3, and a release build of the demo:
#
#     xvfb-run -a bash scripts/verify_showcase.sh
#
set -euo pipefail

cd "$(dirname "$0")/.."

DEMO_BIN="${DEMO_BIN:-./target/release/demo2d}"
FRAMES="${FRAMES:-120}"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"

# shellcheck source=scripts/lib.sh
source scripts/lib.sh
check_demo_environment

mkdir -p artifacts
LOG_FILE="artifacts/showcase.log"

echo "==> showcase: 8 sprites, 4 batches, 4 draw calls"
if ! RUST_LOG="${RUST_LOG:-info}" "$DEMO_BIN" --frames "$FRAMES" \
    --timeout "$TIMEOUT_SECS" --scene showcase \
    --capture artifacts/showcase.ppm --stats >"$LOG_FILE" 2>&1; then
    cat "$LOG_FILE" >&2
    exit 1
fi
expect_log 'assets: .* -> atlas 96x32 \(3 frames\)'
expect_log 'render: scene showcase \| sampler linear \| sprites 8 \| batches 4 \| draw calls 4'

echo "==> showcase: documented pixels in the captured frame"
python3 scripts/verify_showcase.py artifacts/showcase.ppm
echo "    PASS: rotation, tint, alpha, samplers, and PNG alpha all verified"

echo
echo "showcase verification passed"
