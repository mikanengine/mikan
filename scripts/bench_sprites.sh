#!/usr/bin/env bash
#
# Measures the stress scene honestly: for each sprite count and window size it
# runs the demo without vsync for a fixed frame budget and records the demo's
# own frame-time statistics in artifacts/bench/.
#
# Two window sizes are measured because they separate the two costs: at
# 320x200 the fill work is small, so the numbers show CPU-side per-sprite
# cost; at the default 960x600 the sprites cover far more pixels, so the
# numbers include fill/overdraw. The difference is reported, not hidden.
#
# The numbers are only as meaningful as the machine and driver that produced
# them: on a software rasterizer (llvmpipe), fill-bound scenes are much slower
# than on any GPU. State the hardware when quoting results.
#
# Usage:
#     xvfb-run -a bash scripts/bench_sprites.sh [COUNTS] [FRAMES]
#
# Defaults: COUNTS="100 1000 10000", FRAMES=300. SIZES overrides the window
# list, DEMO_BIN the binary.
#
set -euo pipefail

cd "$(dirname "$0")/.."

DEMO_BIN="${DEMO_BIN:-./target/release/demo2d}"
COUNTS="${1:-100 1000 10000}"
FRAMES="${2:-300}"
SIZES="${SIZES:-960x600 320x200}"

# shellcheck source=scripts/lib.sh
source scripts/lib.sh
check_demo_environment

mkdir -p artifacts/bench

for size in $SIZES; do
    for count in $COUNTS; do
        log="artifacts/bench/${size}_${count}.log"
        echo "==> $size window, $count sprites, $FRAMES frames, no vsync"
        if ! RUST_LOG="${RUST_LOG:-info}" "$DEMO_BIN" --scene stress \
            --sprites "$count" --frames "$FRAMES" --no-vsync --size "$size" \
            --stats >"$log" 2>&1; then
            cat "$log" >&2
            exit 1
        fi
    done
done

echo
echo "results (recorded from artifacts/bench/*.log):"
echo
python3 scripts/bench_report.py artifacts/bench/*.log
echo
echo "reminder: software rasterizer numbers unless run on real hardware;"
echo "the README documents how to quote them."
