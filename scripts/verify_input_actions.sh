#!/usr/bin/env bash
#
# Verifies the action-mapping input lane (--actions) end to end: synthetic X11
# key and wheel events must flow through the platform event log into the
# native C backend and C++ action runtime, drive the fixed-step simulation,
# and quit the application.
#
# Checks:
#   1. Holding D moves the square right by roughly speed * hold time, via
#      action state instead of raw key queries.
#   2. Wheel scrolls arrive as vertical-axis action values (the shutdown
#      summary reports wheel ticks).
#   3. A short tap of Escape quits cleanly with exit status 0 through the
#      action lane's press edge (same log line as the raw-key lane).
#
# Requires: a display (Xvfb is fine), xdotool, python3, and a release build of
# the demo:
#
#     xvfb-run -a bash scripts/verify_input_actions.sh
#
set -euo pipefail

cd "$(dirname "$0")/.."

DEMO_BIN="${DEMO_BIN:-./target/release/demo2d}"
FRAME_BUDGET="${FRAME_BUDGET:-3000}"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"

# shellcheck source=scripts/lib.sh
source scripts/lib.sh
check_environment
trap cleanup_demo EXIT

# --- 1. movement through actions ------------------------------------------

echo "==> --actions: holding D for 1 s must move the square right"
start_demo "$FRAME_BUDGET" --actions
xdotool keydown d
sleep 1
xdotool keyup d
wait "$DEMO_PID"

final_position="$(python3 - "$RUN_LOG" <<'PY'
import re, sys
log = open(sys.argv[1], encoding="utf-8", errors="replace").read()
match = re.search(r"square at \[(-?[\d.]+), (-?[\d.]+)\]", log)
if not match:
    sys.exit("no final position in the demo log")
print(match.group(1), match.group(2))
PY
)"
read -r end_x end_y <<<"$final_position"
echo "    square ended at [$end_x, $end_y]"

python3 - "$end_x" "$end_y" <<'PY'
import sys
x, y = float(sys.argv[1]), float(sys.argv[2])
# 420 px/s for ~1 s from the 960-wide window's centre (x = 480). The keyup
# arrives late by a few frames, so allow a wide band below the right wall.
if not 480 + 250 <= x <= 912:
    sys.exit(f"FAIL: square should have moved right to ~900 px, ended at {x}")
if abs(y - 300) > 1:
    sys.exit(f"FAIL: square drifted vertically to {y}")
PY
echo "    PASS: keyboard actions move the simulation"

# --- 2. wheel ticks -------------------------------------------------------

echo "==> --actions: wheel scrolls must reach the vertical wheel binding"
start_demo "$FRAME_BUDGET" --actions
sleep 0.5
xdotool click 4
sleep 0.5
xdotool click 4
sleep 0.5
wait "$DEMO_PID"

python3 - "$RUN_LOG" <<'PY'
import re, sys
log = open(sys.argv[1], encoding="utf-8", errors="replace").read()
match = re.search(r"actions: (\d+) fixed steps, (\d+) moved, (\d+) wheel ticks", log)
if not match:
    sys.exit("no actions summary in the demo log")
steps, moved, ticks = (int(match.group(i)) for i in (1, 2, 3))
if not steps > 0:
    sys.exit(f"FAIL: no fixed steps recorded ({steps})")
if not ticks >= 2:
    sys.exit(f"FAIL: expected at least 2 wheel ticks, got {ticks} in {steps} steps")
PY
echo "    PASS: wheel actions carry the scroll"

# --- 3. escape through the action lane ------------------------------------

echo "==> --actions: tapping Escape must quit cleanly through actions"
start_demo 10000000 --actions
xdotool key Escape
if wait "$DEMO_PID"; then
    status=0
else
    status=$?
fi
[ "$status" -eq 0 ] || {
    echo "FAIL: the demo exited with status $status after Escape" >&2
    tail -5 "$RUN_LOG" >&2
    exit 1
}
grep -q "escape pressed" "$RUN_LOG" || {
    echo "FAIL: the demo exited without reporting the Escape action" >&2
    tail -5 "$RUN_LOG" >&2
    exit 1
}
if grep -q "not producing frames" "$RUN_LOG"; then
    echo "FAIL: the demo hit its frame-budget timeout instead of quitting on Escape" >&2
    exit 1
fi
grep -q "action mapping active" "$RUN_LOG" || {
    echo "FAIL: --actions never initialized the action mapping" >&2
    tail -5 "$RUN_LOG" >&2
    exit 1
}
echo "    PASS: Escape quits through the action mapping"

echo
echo "input (actions) verification passed"
