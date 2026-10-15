#!/usr/bin/env bash
#
# Verifies the 2D camera end to end: the follow camera scrolls the world so the
# player stays centred, and --zoom scales the rendered square.
#
# Checks:
#   1. --camera follow: holding D moves the square in world space (read from the
#      demo log) while the captured frame still shows it at the capture centre,
#      which is only true if the view scrolled with it.
#   2. --camera follow --zoom 2: the square is rendered at twice its size,
#      still centred.
#
# The square is textured, so a screenshot really exercises the texture path as
# well; see docs/adr/0006 for what this does and does not prove.
#
# Requires: a display (Xvfb is fine), xdotool, python3, and a release build of
# the demo:
#
#     xvfb-run -a bash scripts/verify_camera.sh
#
set -euo pipefail

cd "$(dirname "$0")/.."

DEMO_BIN="${DEMO_BIN:-./target/release/demo2d}"
FRAME_BUDGET="${FRAME_BUDGET:-10000000}"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"

# shellcheck source=scripts/lib.sh
source scripts/lib.sh
check_environment
trap cleanup_demo EXIT

mkdir -p artifacts

# --- 1. follow camera -----------------------------------------------------

echo "==> follow camera: the world moves, the square stays centred"
start_demo "$FRAME_BUDGET" --camera follow --capture artifacts/camera_follow.ppm
xdotool keydown d
sleep 1
xdotool keyup d
sleep 0.3
xdotool key Escape
wait "$DEMO_PID"

world_position="$(python3 - "$RUN_LOG" <<'PY'
import re, sys
log = open(sys.argv[1], encoding="utf-8", errors="replace").read()
match = re.search(r"square at \[(-?[\d.]+), (-?[\d.]+)\]", log)
if not match:
    sys.exit("no final position in the demo log")
print(match.group(1), match.group(2))
PY
)"
read -r world_x world_y <<<"$world_position"
echo "    square ended at world [$world_x, $world_y]"

python3 - "$world_x" "$world_y" <<'PY'
import sys
x, y = float(sys.argv[1]), float(sys.argv[2])
# Same band the input verification uses: 420 px/s for ~1 s from the 960-wide
# window's centre (x = 480), with the keyup arriving a few frames late.
if not 480 + 250 <= x <= 912:
    sys.exit(f"FAIL: the square should have moved right to ~900 px, ended at {x}")
if abs(y - 300) > 1:
    sys.exit(f"FAIL: the square drifted vertically to {y}")
PY

python3 scripts/verify_camera.py artifacts/camera_follow.ppm --size 96
echo "    PASS: the camera followed the moving square"

# --- 2. zoom --------------------------------------------------------------

echo "==> zoom 2: the square renders at twice its size"
# No input is needed for this check, so the demo is not focused and does not
# have to outlive the window search: it just presents its frame budget and
# writes the capture.
: >"$RUN_LOG"
"$DEMO_BIN" --frames 120 --timeout "$TIMEOUT_SECS" \
    --camera follow --zoom 2 --capture artifacts/camera_zoom.ppm >>"$RUN_LOG" 2>&1 &
DEMO_PID=$!
wait "$DEMO_PID"
python3 scripts/verify_camera.py artifacts/camera_zoom.ppm --size 192
echo "    PASS: zoom scales the rendered world"

echo
echo "camera verification passed"
