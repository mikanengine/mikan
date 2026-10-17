#!/usr/bin/env bash
#
# Verifies the scene & transform-hierarchy milestone end to end (ADR 0012):
# the `--scene hierarchy` demonstration is a real entity hierarchy whose rigid
# rotation and deferred despawn are visible in captured frames.
#
# Checks:
#   1. The startup log reports the hierarchy's bodies and the spin system.
#   2. With no input, the sun, the first planet, and the moon are drawn at
#      their exact documented pixels (scripts/verify_scene.py layout).
#   3. With a spin key held, the moon stays exactly one orbit radius from the
#      sun while its angle changes: the hierarchy rotates rigidly (orbit).
#   4. After F, the moon's pixels are gone and the planets remain (despawn).
#
# Requires: a display (Xvfb is fine), xdotool, python3, and a release build of
# the demo:
#
#     xvfb-run -a bash scripts/verify_scene.sh
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

# --- 1 + 2. no input: the documented initial layout ------------------------

echo "==> hierarchy: the startup log reports the bodies and the spin system"
start_demo "$FRAME_BUDGET" --scene hierarchy --capture artifacts/scene_layout.ppm
LOG_FILE="$RUN_LOG"
expect_log "hierarchy: sun .* \+ .* \+ moon .* \| orbit 160, moon orbit 80"
expect_log "ecs: 5 entities, 5 components, systems: spin"
xdotool key Escape
wait "$DEMO_PID"

echo "==> hierarchy: the initial sun, planet, and moon pixels are exact"
python3 scripts/verify_scene.py layout artifacts/scene_layout.ppm

# --- 3. spin: rotation stays rigid -----------------------------------------

echo "==> hierarchy: holding D spins the sun and carries the moon around"
start_demo "$FRAME_BUDGET" --scene hierarchy --capture artifacts/scene_orbit.ppm
LOG_FILE="$RUN_LOG"
xdotool keydown d
sleep 0.6
xdotool keyup d
sleep 0.2
xdotool key Escape
wait "$DEMO_PID"
python3 scripts/verify_scene.py orbit artifacts/scene_orbit.ppm

# --- 4. despawn: the moon leaves, the planets stay -------------------------

echo "==> hierarchy: F despawns the moon without disturbing the planets"
start_demo "$FRAME_BUDGET" --scene hierarchy --capture artifacts/scene_despawn.ppm
LOG_FILE="$RUN_LOG"
xdotool keydown f
sleep 0.2
xdotool keyup f
sleep 0.3
xdotool key Escape
wait "$DEMO_PID"
expect_log "F pressed; despawning the moon"
python3 scripts/verify_scene.py despawn artifacts/scene_despawn.ppm

echo
echo "scene verification passed"
