#!/usr/bin/env bash
#
# Measures the C narrow-phase kernel against the Rust reference
# implementation honestly: the same pair set runs through both, in release
# mode, with warm-ups and repeated runs, and the tool prints nanoseconds per
# pair and the ratio. See crates/mikan-collision/examples/bench.rs for the
# generator and profiling discipline, and docs/collision.md for how to read
# the numbers.
#
# The result is written to artifacts/bench/collision_<pairs>.log.
#
# Usage:
#     bash scripts/bench_collision.sh [PAIR_COUNT]
#
# Defaults: PAIR_COUNT=250000. The demo binary and display are not needed;
# this measures CPU work only.
#
set -euo pipefail

cd "$(dirname "$0")/.."

PAIRS="${1:-250000}"

mkdir -p artifacts/bench
LOG="artifacts/bench/collision_${PAIRS}.log"

echo "==> $PAIRS pairs through the C kernel and the Rust reference, release mode"
cargo run -q --release -p mikan-collision --example bench -- "$PAIRS" | tee "$LOG"

echo
echo "reminder: the two implementations are expected to be close; the kernel's"
echo "value is its portable ABI, not raw speed. Quote the machine when quoting"
echo "the numbers."
