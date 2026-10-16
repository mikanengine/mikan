#!/usr/bin/env bash
#
# Verifies pixel-perfect mode end to end: --pixel-perfect 240x150 in the
# default 960x600 window derives the integer zoom 4 (reported in the demo
# log), and the captured player square renders exactly 4 physical pixels per
# virtual pixel with every edge on the physical pixel grid.
#
# The virtual size and window can be overridden with VIRTUAL and SIZE, but
# then the expected zoom (window / virtual, floored, at least 1) and square
# size (96 virtual pixels, PLAYER_SIZE in games/demo2d/src/scene.rs) must be
# passed through ZOOM and SQUARE.
#
# Requires: a display (Xvfb is fine), python3, and a release build of the demo:
#
#     xvfb-run -a bash scripts/verify_pixel_perfect.sh
#
set -euo pipefail

cd "$(dirname "$0")/.."

DEMO_BIN="${DEMO_BIN:-./target/release/demo2d}"
FRAMES="${FRAMES:-120}"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"
VIRTUAL="${VIRTUAL:-240x150}"
SIZE="${SIZE:-960x600}"
ZOOM="${ZOOM:-4}"
SQUARE="${SQUARE:-384}"

# shellcheck source=scripts/lib.sh
source scripts/lib.sh
check_demo_environment

mkdir -p artifacts
LOG_FILE="artifacts/pixel_perfect.log"

echo "==> pixel-perfect: virtual $VIRTUAL in a ${SIZE} window, integer zoom x$ZOOM"
if ! RUST_LOG="${RUST_LOG:-info}" "$DEMO_BIN" --frames "$FRAMES" \
    --timeout "$TIMEOUT_SECS" --size "$SIZE" --pixel-perfect "$VIRTUAL" \
    --capture artifacts/pixel_perfect.ppm --stats >"$LOG_FILE" 2>&1; then
    cat "$LOG_FILE" >&2
    exit 1
fi
expect_log "pixel-perfect: virtual $VIRTUAL, integer zoom x$ZOOM"

echo "==> pixel-perfect: the player square is $SQUARE px, aligned to the grid"
python3 scripts/verify_pixel_perfect.py artifacts/pixel_perfect.ppm \
    --scale "$ZOOM" --size "$SQUARE"
echo "    PASS: integer zoom and physical-grid alignment verified"

echo
echo "pixel-perfect verification passed"
