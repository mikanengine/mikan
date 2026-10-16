#!/usr/bin/env bash
#
# Verifies the collision milestone end to end: the classic scene's blocks are
# real `mikan-collision` bodies in the ECS, the `collide` system resolves the
# player against them once per fixed step, and the resolution is positional.
#
# Checks:
#   1. The startup log reports the three static blocks and the broad-phase
#      grid, next to the five-system schedule.
#   2. Holding down+right drives the player into the first pillar's left
#      face: the run must stop at x = 582 (the face minus the player's half
#      width) and the held keys must slide the player down the face to the
#      floor clamp at y = 552. Passing through, stopping at first contact, or
#      being ejected along the wrong axis all fail this.
#   3. A straight run along y = 300 is unobstructed: the block layout must
#      leave the corridor the input and camera verifications walk along.
#
# Requires: a display (Xvfb is fine), xdotool, python3, and a release build of
# the demo:
#
#     xvfb-run -a bash scripts/verify_collision.sh
#
set -euo pipefail

cd "$(dirname "$0")/.."

DEMO_BIN="${DEMO_BIN:-./target/release/demo2d}"
FRAME_BUDGET="${FRAME_BUDGET:-10000000}"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"

# The classic scene's geometry (games/demo2d/src/): the first block spans
# x 630..770, so its left face minus the 96-pixel player's half width is 582;
# the floor clamp for the 600-pixel world is 600 - 48 = 552.
FACE_X=582
FLOOR_Y=552

# shellcheck source=scripts/lib.sh
source scripts/lib.sh
check_environment
trap cleanup_demo EXIT

# --- 1. the level and the schedule ----------------------------------------

echo "==> the startup log must list the blocks, the grid, and the collide step"
start_demo "$FRAME_BUDGET"
LOG_FILE="$RUN_LOG"
expect_log "ecs: 4 entities, 11 components, systems: snapshot, input, movement, collide, clamp"
expect_log "collision: 3 static blocks, grid cell 160"

# --- 2. the diagonal run stops against the pillar -------------------------

echo "==> holding down+right must stop against the pillar's face and slide"
xdotool keydown d
xdotool keydown s
sleep 2
xdotool keyup d
xdotool keyup s
sleep 0.3
xdotool key Escape
wait "$DEMO_PID"

python3 - "$RUN_LOG" "$FACE_X" "$FLOOR_Y" <<'PY'
import re, sys

log = open(sys.argv[1], encoding="utf-8", errors="replace").read()
face_x, floor_y = float(sys.argv[2]), float(sys.argv[3])
square = re.search(r"square at \[(-?[\d.]+), (-?[\d.]+)\]", log)
if not square:
    sys.exit("no final position in the demo log")
x, y = float(square.group(1)), float(square.group(2))
print(f"    square ended at [{x}, {y}] (expected face x {face_x}, floor y {floor_y})")
if abs(x - face_x) > 1:
    sys.exit(
        f"FAIL: the diagonal run should have stopped against the pillar's "
        f"left face at x = {face_x}, ended at x = {x}"
    )
if abs(y - floor_y) > 1:
    sys.exit(
        f"FAIL: the run should have slid down the face to the floor clamp "
        f"at y = {floor_y}, ended at y = {y}"
    )
PY
echo "    PASS: the pillar blocked the diagonal run and the player slid along it"

# --- 3. the corridor stays clear ------------------------------------------

echo "==> a straight run along y = 300 must be unobstructed"
start_demo "$FRAME_BUDGET"
xdotool keydown d
sleep 1
xdotool keyup d
sleep 0.3
xdotool key Escape
wait "$DEMO_PID"

python3 - "$RUN_LOG" <<'PY'
import re, sys

log = open(sys.argv[1], encoding="utf-8", errors="replace").read()
square = re.search(r"square at \[(-?[\d.]+), (-?[\d.]+)\]", log)
if not square:
    sys.exit("no final position in the demo log")
x, y = float(square.group(1)), float(square.group(2))
print(f"    square ended at [{x}, {y}]")
# The same band the input verification uses: 420 px/s for ~1 s from the
# 960-wide window's centre, with the keyup arriving a few frames late.
if not 480 + 250 <= x <= 912:
    sys.exit(f"FAIL: the straight run should have reached the right wall band, ended at {x}")
if abs(y - 300) > 1:
    sys.exit(f"FAIL: the block layout deflected the straight run to y = {y}")
PY
echo "    PASS: the blocks leave the documented corridor untouched"

echo
echo "collision verification passed"
