#!/usr/bin/env bash
#
# Measures the native input pipeline (C backend + C++ action runtime)
# against the Rust Normalizer reference honestly: the same deterministic
# event batches run through both, in release mode, with warm-ups and
# repeated runs. See crates/input/examples/bench_input.rs for the generator
# and profiling discipline, and docs/input.md for how to read the numbers.
#
# The result is written to artifacts/bench/input.log.
#
# Usage:
#     bash scripts/bench_input.sh [BATCHES]
#
# Defaults: BATCHES=65536. The demo binary and display are not needed; this
# measures CPU work only.
#
set -euo pipefail

cd "$(dirname "$0")/.."

BATCHES="${1:-65536}"

mkdir -p artifacts/bench
LOG="artifacts/bench/input_${BATCHES}.log"

echo "==> $BATCHES event batches through the native pipeline and the Rust reference, release mode"
cargo run -q --release -p mikan-input --example bench_input -- "$BATCHES" | tee "$LOG"

echo
echo "reminder: the engine-step row is the number a game cares about; the"
echo "kernel/reference rows exist to show the normalization cost alone. Quote"
echo "the machine when quoting the numbers."
