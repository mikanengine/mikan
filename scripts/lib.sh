#!/usr/bin/env bash
#
# Shared helpers for the X11 end-to-end verification scripts (verify_input.sh,
# verify_camera.sh). Sourced, not executed directly.
#
# The sourcing script must cd to the repository root, set DEMO_BIN and
# TIMEOUT_SECS, then source this file, call check_environment, install
# `trap cleanup_demo EXIT`, and use start_demo to launch the demo.

RUN_LOG=""
WINDOW_ID=""
DEMO_PID=""

# Verifies the required tools, a display, and the demo binary.
check_environment() {
    for tool in xdotool python3; do
        command -v "$tool" >/dev/null 2>&1 || {
            echo "error: $tool is required but not installed" >&2
            exit 2
        }
    done
    [ -n "${DISPLAY:-}" ] || {
        echo "error: no DISPLAY; run under xvfb-run -a or on a desktop session" >&2
        exit 2
    }
    [ -x "$DEMO_BIN" ] || {
        echo "error: $DEMO_BIN not found; run cargo build --release -p demo2d" >&2
        exit 2
    }
    RUN_LOG="$(mktemp)"
}

# Kills only the demo process this script started. Matching by command line
# (`pkill -f`) would also kill unrelated processes — such as the shell that
# invoked the verification — whose arguments merely mention the binary.
cleanup_demo() {
    [ -n "$DEMO_PID" ] || return 0
    kill "$DEMO_PID" >/dev/null 2>&1 || true
}

find_window() {
    for _ in $(seq 1 100); do
        WINDOW_ID="$(xdotool search --name 'Mikan' 2>/dev/null | tail -1 || true)"
        if [ -n "$WINDOW_ID" ]; then
            return 0
        fi
        sleep 0.1
    done
    echo "error: the demo window did not appear" >&2
    return 1
}

# Focus is only guaranteed once the window is mapped, and a key sent before that
# goes nowhere. Set focus, then confirm the X server agrees before typing.
focus_window() {
    for _ in $(seq 1 50); do
        # A demo that exited on its own (for example after a small --frames
        # budget) can never be focused; fail fast instead of retrying for 5 s.
        kill -0 "$DEMO_PID" 2>/dev/null || break
        xdotool windowfocus --sync "$WINDOW_ID" >/dev/null 2>&1 || true
        if [ "$(xdotool getwindowfocus 2>/dev/null || true)" = "$WINDOW_ID" ]; then
            return 0
        fi
        sleep 0.1
    done
    echo "error: could not focus the demo window" >&2
    return 1
}

# Starts the demo with a frame budget and optional extra arguments, waits for
# the window, and gives it focus. Sets DEMO_PID for `wait` and `cleanup_demo`.
start_demo() {
    local frames=$1
    shift
    : >"$RUN_LOG"
    "$DEMO_BIN" --frames "$frames" --timeout "$TIMEOUT_SECS" "$@" >>"$RUN_LOG" 2>&1 &
    DEMO_PID=$!
    find_window
    sleep 0.3
    focus_window
    sleep 0.2
}
