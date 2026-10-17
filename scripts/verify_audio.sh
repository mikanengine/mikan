#!/usr/bin/env bash
#
# Verifies the audio system end to end: the demo's `--audio` lane opens the
# mixer (the real output device when one exists, the deterministic offline
# ring otherwise), loads blip.wav through the real asset pipeline, plays it
# on the movement edges synthetic keys produce, and reports honest counters.
#
# Checks:
#   1. The lane starts, says which backend it got, and decoded the generator's
#      blip (5760 frames at 48 kHz, mono).
#   2. Pressing D then W starts exactly one voice each, the second at the up
#      pitch (1.25), and both voices finish.
#   3. On the offline path the ring never overran (the demo drains every
#      frame), nothing logged an audio failure, and Escape quits cleanly.
#
# Requires: a display (Xvfb is fine), xdotool, python3, and a release build of
# the demo:
#
#     xvfb-run -a bash scripts/verify_audio.sh
#
set -euo pipefail

cd "$(dirname "$0")/.."

DEMO_BIN="${DEMO_BIN:-./target/release/demo2d}"
FRAME_BUDGET="${FRAME_BUDGET:-6000}"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"

# shellcheck source=scripts/lib.sh
source scripts/lib.sh
check_environment
LOG_FILE="$RUN_LOG"
trap cleanup_demo EXIT

# --- 1. the lane starts and the blip decodes ------------------------------

echo "==> the audio lane must open a backend and decode blip.wav"
start_demo "$FRAME_BUDGET" --audio
expect_log "audio: (device backend|no usable output device)"
expect_log "audio: .*blip\.wav -> 5760 frames at 48000 Hz, 1 channel\(s\)"
expect_log "audio: the blip plays when movement starts"
echo "    PASS: the lane is up and the blip went through the asset pipeline"

# --- 2. two edges, two voices, the second pitched up ----------------------

echo "==> D then W must start one blip each, the second at the up pitch"
xdotool keydown d
sleep 0.6
xdotool keyup d
sleep 0.3
xdotool keydown w
sleep 0.6
xdotool keyup w
sleep 0.3
expect_log "audio: blip #1 at pitch 1\.00"
expect_log "audio: blip #2 at pitch 1\.25"

# --- 3. clean shutdown with coherent counters -----------------------------

echo "==> Escape must quit cleanly and the counters must add up"
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
    echo "FAIL: the demo exited without reporting the Escape key" >&2
    tail -5 "$RUN_LOG" >&2
    exit 1
}
if grep -q "audio: could not" "$RUN_LOG"; then
    echo "FAIL: the audio lane reported a failure" >&2
    grep "audio: could not" "$RUN_LOG" >&2
    exit 1
fi

python3 - "$RUN_LOG" <<'PY'
import re, sys

log = open(sys.argv[1], encoding="utf-8", errors="replace").read()
pattern = (
    r"audio: (\d+) plays, (\d+) voices started, (\d+) finished, "
    r"(\d+) blocks mixed, (\d+) overruns, backend (\S+)"
)
match = re.search(pattern, log)
if not match:
    sys.exit("no audio summary in the demo log")
plays, started, finished, blocks, overruns, backend = (
    int(match.group(1)),
    int(match.group(2)),
    int(match.group(3)),
    int(match.group(4)),
    int(match.group(5)),
    match.group(6),
)
if plays != 2:
    sys.exit(f"FAIL: expected 2 plays (D then W), saw {plays}")
if started != plays:
    sys.exit(f"FAIL: {started} voices started for {plays} plays")
if finished != started:
    sys.exit(f"FAIL: {finished} of {started} voices finished before shutdown")
if overruns != 0:
    sys.exit(f"FAIL: {overruns} offline blocks overran; the ring should drain every frame")
if blocks < plays:
    sys.exit(f"FAIL: only {blocks} blocks mixed for {plays} plays")
print(
    f"    backend {backend}: {plays} plays, {started} started, "
    f"{finished} finished, {blocks} blocks mixed, {overruns} overruns"
)
PY
echo "    PASS: the blips played, mixed, and finished; no overruns"

# --- 4. a second run proves the counter is not a constant ------------------

echo "==> one key press must report one play"
start_demo "$FRAME_BUDGET" --audio
expect_log "audio: the blip plays when movement starts"
xdotool keydown a
sleep 0.6
xdotool keyup a
sleep 0.3
xdotool key Escape
if wait "$DEMO_PID"; then
    status=0
else
    status=$?
fi
[ "$status" -eq 0 ] || {
    echo "FAIL: the second run exited with status $status" >&2
    tail -5 "$RUN_LOG" >&2
    exit 1
}
grep -qE "audio: 1 plays, 1 voices started" "$RUN_LOG" || {
    echo "FAIL: the second run did not report exactly one play" >&2
    grep "audio: " "$RUN_LOG" | tail -3 >&2
    exit 1
}
echo "    PASS: the play counter tracks the input edges"

echo
echo "audio verification passed"
