#!/usr/bin/env bash
#
# Verifies the ECS-lite milestone end to end: the classic scene's player is a
# `mikan-ecs` entity, its four named systems run on the engine's fixed step,
# and movement advances exactly one fixed step at a time.
#
# Checks:
#   1. The startup log reports one entity, its four components, and the system
#      order (snapshot, input, movement, clamp).
#   2. Holding D for ~0.5 s moves the player exactly 3.5 px per *moved* fixed
#      step (420 px/s at the default 120 Hz). The shutdown log's moved-step
#      counter and the final position must agree, which is what "scheduled on
#      the fixed timestep" means: one update per step, no double updates.
#   3. With no input at all, nothing moves: the position stays at the start
#      centre and the moved-step counter stays zero.
#
# Requires: a display (Xvfb is fine), xdotool, python3, and a release build of
# the demo:
#
#     xvfb-run -a bash scripts/verify_ecs.sh
#
set -euo pipefail

cd "$(dirname "$0")/.."

DEMO_BIN="${DEMO_BIN:-./target/release/demo2d}"
FRAME_BUDGET="${FRAME_BUDGET:-10000000}"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"

# The classic scene's geometry: the 960-wide window's centre, PLAYER_SPEED
# (games/demo2d/src/scene.rs) divided by the default 120 Hz fixed step, and the
# right wall the clamp keeps the 96 px player inside.
START_X=480
STEP_PX=3.5
RIGHT_WALL=912

# shellcheck source=scripts/lib.sh
source scripts/lib.sh
check_environment
trap cleanup_demo EXIT

# --- 1 + 2. systems run once per fixed step -------------------------------

echo "==> the startup log must list the player entity and its systems"
start_demo "$FRAME_BUDGET"
LOG_FILE="$RUN_LOG"
expect_log "ecs: 1 entity, 4 components, systems: snapshot, input, movement, clamp"

echo "==> holding D for 0.5 s must move exactly 3.5 px per moved fixed step"
xdotool keydown d
sleep 0.5
xdotool keyup d
sleep 0.3
xdotool key Escape
wait "$DEMO_PID"

python3 - "$RUN_LOG" "$START_X" "$STEP_PX" "$RIGHT_WALL" <<'PY'
import re, sys

log = open(sys.argv[1], encoding="utf-8", errors="replace").read()
start_x, step_px, wall = float(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
square = re.search(r"square at \[(-?[\d.]+), (-?[\d.]+)\]", log)
steps = re.search(r"ecs: 1 entity, (\d+) fixed steps, (\d+) moved steps", log)
if not square:
    sys.exit("no final position in the demo log")
if not steps:
    sys.exit("no ECS step summary in the demo log")
x, y = float(square.group(1)), float(square.group(2))
fixed, moved = int(steps.group(1)), int(steps.group(2))
print(f"    moved steps: {moved} of {fixed} fixed steps; square at [{x}, {y}]")
if moved <= 0:
    sys.exit("FAIL: holding D did not move the player")
if moved > fixed:
    sys.exit(f"FAIL: {moved} moved steps out of {fixed} fixed steps")
# Every moved step advanced exactly 3.5 px from the start centre, until the
# right wall pins the player at 912. The step that reaches the wall counts as
# moved; steps pinned against it do not change the position, so they do not.
expected = min(start_x + step_px * moved, wall)
if abs(x - expected) > 1:
    sys.exit(
        f"FAIL: x={x} is not {moved} fixed steps of {step_px} px "
        f"from {start_x} (expected {expected})"
    )
if abs(y - 300) > 1:
    sys.exit(f"FAIL: the player drifted vertically to {y}")
PY
echo "    PASS: one fixed step per moved step, no double updates"

# --- 3. no input, no movement ---------------------------------------------

echo "==> with no input the player must not move at all"
: >"$RUN_LOG"
"$DEMO_BIN" --frames 120 --timeout "$TIMEOUT_SECS" >>"$RUN_LOG" 2>&1 &
DEMO_PID=$!
wait "$DEMO_PID"
LOG_FILE="$RUN_LOG"
expect_log "square at \[480, 300\]"
expect_log "ecs: 1 entity, [0-9]+ fixed steps, 0 moved steps"
echo "    PASS: no input means no movement and no moved steps"

echo
echo "ecs verification passed"
