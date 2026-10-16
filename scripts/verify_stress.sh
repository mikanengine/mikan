#!/usr/bin/env bash
#
# Verifies the stress scene end to end: many sprites from one texture really
# render as a single batch and a single draw call, and the captured frame is
# actually full of textured sprites.
#
# Checks:
#   1. The demo log reports the requested sprite count and render stats of
#      exactly 1 batch and 1 draw call.
#   2. The captured frame is mostly covered in the player tile's colours
#      (scripts/verify_stress.py).
#
# The default count is 1000 so CI stays fast; the 10,000-sprite benchmark is
# scripts/bench_sprites.sh, which measures timings instead of correctness.
#
# Requires: a display (Xvfb is fine), python3, and a release build of the demo:
#
#     xvfb-run -a bash scripts/verify_stress.sh
#
set -euo pipefail

cd "$(dirname "$0")/.."

DEMO_BIN="${DEMO_BIN:-./target/release/demo2d}"
FRAMES="${FRAMES:-120}"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"
SPRITES="${SPRITES:-1000}"

# shellcheck source=scripts/lib.sh
source scripts/lib.sh
check_demo_environment

mkdir -p artifacts
LOG_FILE="artifacts/stress.log"

echo "==> stress: $SPRITES sprites in one batch and one draw call"
if ! RUST_LOG="${RUST_LOG:-info}" "$DEMO_BIN" --frames "$FRAMES" \
    --timeout "$TIMEOUT_SECS" --scene stress --sprites "$SPRITES" \
    --capture artifacts/stress.ppm --stats >"$LOG_FILE" 2>&1; then
    cat "$LOG_FILE" >&2
    exit 1
fi
expect_log "stress: $SPRITES sprites"
expect_log "render: scene stress \| sampler linear \| sprites $SPRITES \| batches 1 \| draw calls 1"

echo "==> stress: the captured frame is full of textured sprites"
python3 scripts/verify_stress.py artifacts/stress.ppm --min-warm 0.15 --min-covered 0.20
echo "    PASS: batching held and the sprites were rasterized"

echo
echo "stress verification passed"
