#!/usr/bin/env bash
#
# Verifies the Luau gameplay-scripting milestone end to end (ADR 0011): the
# demo's default script drives the player's movement through `mikan-script`,
# and every documented failure path behaves exactly as recorded.
#
# Checks:
#   1. The startup log reports the loaded script next to the ECS facts.
#   2. Holding D moves the player exactly 3.5 px per moved fixed step: the
#      default script returns the same direction and speed the Rust code used
#      to (a 420 px/s axis move), so movement now *is* the script.
#   3. Engine-side normalization: a script returning (3, 4, 60) for exactly
#      60 steps must trace the (0.6, 0.8) direction — the player ends at
#      (480 + 18, 300 + 24) = [498, 324]. Unnormalized it would trace
#      (1.5, 2.0) px per step and end at [570, 420]; with the counter broken
#      it would clamp at [912, 552]. All three are distinguishable.
#   4. A syntax error is fatal at startup, with the file and line.
#   5. A script without `mikan_step` is fatal at startup.
#   6. A missing script file is fatal at startup.
#   7. A runtime error inside `mikan_step` is logged exactly once and the
#      game keeps running with the player idling (0 moved steps).
#   8. A non-finite return is rejected as an invalid step: logged once, the
#      game keeps running, nothing moves.
#
# Requires: a display (Xvfb is fine), xdotool, python3, and a release build of
# the demo:
#
#     xvfb-run -a bash scripts/verify_script.sh
#
set -euo pipefail

cd "$(dirname "$0")/.."

DEMO_BIN="${DEMO_BIN:-./target/release/demo2d}"
FRAME_BUDGET="${FRAME_BUDGET:-10000000}"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"
DEFAULT_SCRIPT="games/demo2d/scripts/player.luau"

# The classic scene's geometry, as in verify_ecs.sh: the window centre, the
# default script's speed (420 px/s at 120 Hz = 3.5 px per moved step), the
# right wall, and the 60-step normalization target.
START_X=480
STEP_PX=3.5
RIGHT_WALL=912
NORM_X=498
NORM_Y=324

FIXTURE_DIR="$(mktemp -d)"

# shellcheck source=scripts/lib.sh
source scripts/lib.sh
check_environment

cleanup_all() {
    cleanup_demo
    rm -rf "$FIXTURE_DIR"
}
trap cleanup_all EXIT

# Fixture scripts for the failure paths.
cat >"$FIXTURE_DIR/syntax.luau" <<'LUA'
local x = 1
function mikan_step(a b)
end
LUA
cat >"$FIXTURE_DIR/no_step.luau" <<'LUA'
local x = 1
LUA
cat >"$FIXTURE_DIR/boom.luau" <<'LUA'
function mikan_step(dt, left, right, up, down, pos_x, pos_y)
    error("boom")
end
LUA
cat >"$FIXTURE_DIR/nan.luau" <<'LUA'
function mikan_step(dt, left, right, up, down, pos_x, pos_y)
    return 0/0, 0, 0
end
LUA
cat >"$FIXTURE_DIR/normalize.luau" <<'LUA'
-- Exactly 60 steps of (3, 4, 60), then stand still. The engine must
-- normalize the direction to (0.6, 0.8): per step (0.3, 0.4) px, so the
-- player ends at (480 + 18, 300 + 24) = [498, 324]. Without normalization
-- the same 60 steps would be (1.5, 2.0) px each and end at [570, 420].
local steps = 0
function mikan_step(dt, left, right, up, down, pos_x, pos_y)
    steps = steps + 1
    if steps > 60 then
        return 0, 0, 0
    end
    return 3, 4, 60
end
LUA

# --- 1 + 2. the default script drives the documented movement --------------

echo "==> the startup log must report the default script, and D must move at 3.5 px per step"
start_demo "$FRAME_BUDGET"
LOG_FILE="$RUN_LOG"
expect_log "script: $DEFAULT_SCRIPT \(mikan_step\)"
expect_log "ecs: 4 entities, 11 components, systems: snapshot, input, movement, collide, clamp"

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
steps = re.search(r"ecs: 4 entities, (\d+) fixed steps, (\d+) moved steps", log)
if not square or not steps:
    sys.exit("no final position or step summary in the demo log")
x, y = float(square.group(1)), float(square.group(2))
fixed, moved = int(steps.group(1)), int(steps.group(2))
print(f"    moved steps: {moved} of {fixed} fixed steps; square at [{x}, {y}]")
if moved <= 0:
    sys.exit("FAIL: holding D did not move the player; the default script is not driving input")
if moved > fixed:
    sys.exit(f"FAIL: {moved} moved steps out of {fixed} fixed steps")
expected = min(start_x + step_px * moved, wall)
if abs(x - expected) > 1:
    sys.exit(
        f"FAIL: x={x} is not {moved} fixed steps of {step_px} px "
        f"from {start_x} (expected {expected})"
    )
if abs(y - 300) > 1:
    sys.exit(f"FAIL: the player drifted vertically to {y}")
PY
echo "    PASS: the default Luau script reproduces the documented movement"

# --- 3. the engine normalizes over-long directions -------------------------

echo "==> a (3, 4, 60) script must trace the normalized (0.6, 0.8) path"
start_demo "$FRAME_BUDGET" --script "$FIXTURE_DIR/normalize.luau"
LOG_FILE="$RUN_LOG"
expect_log "script: .*normalize\.luau \(mikan_step\)"
# The script stops moving after 60 steps; ~3 s is many times that even on a
# slow machine, so the final position must be the 60-step one, not further.
sleep 3
xdotool key Escape
wait "$DEMO_PID"

python3 - "$RUN_LOG" "$START_X" "$NORM_X" "$NORM_Y" <<'PY'
import re, sys

log = open(sys.argv[1], encoding="utf-8", errors="replace").read()
start_x, norm_x, norm_y = float(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
square = re.search(r"square at \[(-?[\d.]+), (-?[\d.]+)\]", log)
steps = re.search(r"ecs: 4 entities, (\d+) fixed steps, (\d+) moved steps", log)
if not square or not steps:
    sys.exit("no final position or step summary in the demo log")
x, y = float(square.group(1)), float(square.group(2))
fixed, moved = int(steps.group(1)), int(steps.group(2))
print(f"    moved steps: {moved} of {fixed} fixed steps; square at [{x}, {y}]")
if fixed < 60:
    sys.exit(f"FAIL: only {fixed} fixed steps; the run cannot show the 60-step stop")
if moved != 60:
    sys.exit(f"FAIL: the script moves for exactly 60 steps, but {moved} steps moved")
if abs(x - norm_x) > 0.75 or abs(y - norm_y) > 0.75:
    sys.exit(
        f"FAIL: the normalized 60-step path ends at [{norm_x}, {norm_y}]; "
        f"the player ended at [{x}, {y}] "
        f"(unnormalized it would end at [570, 420]; without the 60-step "
        f"stop it would clamp at [912, 552])"
    )
PY
echo "    PASS: the over-long direction was normalized, not scaled up"

# --- 4-6. load-time failures are fatal -------------------------------------

run_expect_fatal() {
    local label=$1 script=$2 pattern=$3
    echo "==> $label must fail startup with a clear error"
    : >"$RUN_LOG"
    set +e
    "$DEMO_BIN" --frames 20 --timeout "$TIMEOUT_SECS" --script "$script" \
        >>"$RUN_LOG" 2>&1
    local status=$?
    set -e
    LOG_FILE="$RUN_LOG"
    if [ "$status" -eq 0 ]; then
        echo "error: $label exited 0; expected a fatal startup error" >&2
        cat "$RUN_LOG" >&2
        exit 1
    fi
    expect_log "$pattern"
    echo "    PASS: $label is fatal (exit $status)"
}

run_expect_fatal "a syntax error" "$FIXTURE_DIR/syntax.luau" \
    "failed to load: syntax error: .*syntax\.luau\"?\]:2:"
run_expect_fatal "a script without mikan_step" "$FIXTURE_DIR/no_step.luau" \
    "script .*no_step\.luau does not define a global function \`mikan_step\`"
run_expect_fatal "a missing script file" "$FIXTURE_DIR/absent.luau" \
    "could not read script .*absent\.luau"

# --- 7 + 8. run-time failures are loud, non-fatal, and once-only -----------

run_expect_survivable() {
    local label=$1 script=$2 pattern=$3
    echo "==> $label must be logged once and leave the game running"
    : >"$RUN_LOG"
    set +e
    "$DEMO_BIN" --frames 120 --timeout "$TIMEOUT_SECS" --script "$script" \
        >>"$RUN_LOG" 2>&1
    local status=$?
    set -e
    LOG_FILE="$RUN_LOG"
    if [ "$status" -ne 0 ]; then
        echo "error: $label killed the run (exit $status)" >&2
        cat "$RUN_LOG" >&2
        exit 1
    fi
    expect_log "$pattern"
    local errors
    errors="$(grep -c "script error:" "$RUN_LOG" || true)"
    if [ "$errors" -ne 1 ]; then
        echo "error: expected the script error to be logged once, saw $errors" >&2
        cat "$RUN_LOG" >&2
        exit 1
    fi
    expect_log "ecs: 4 entities, [0-9]+ fixed steps, 0 moved steps"
    echo "    PASS: $label was logged once and nothing moved"
}

run_expect_survivable "a runtime error in mikan_step" "$FIXTURE_DIR/boom.luau" \
    "script error: script .*boom\.luau failed in mikan_step: runtime error: .*boom\.luau\"?\]:2: boom"
run_expect_survivable "a non-finite return" "$FIXTURE_DIR/nan.luau" \
    "script error: script .*nan\.luau returned an invalid step: the step returned a non-finite value"

echo
echo "script verification passed"
