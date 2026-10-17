#!/usr/bin/env bash
#
# Measures the native scene graph (C++ topology + C compose kernel) against a
# Rust reference fold honestly: the same hierarchy and local transforms run
# through both, in release mode, with warm-ups and repeated runs, and the tool
# prints nanoseconds per node and the ratio. See crates/scene/examples/bench_scene.rs
# for the generator and profiling discipline, and docs/scene.md for how to read
# the numbers.
#
# The result is written to artifacts/bench/scene_<nodes>.log.
#
# Usage:
#     bash scripts/bench_scene.sh [NODE_COUNT]
#
# Defaults: NODE_COUNT=100000. The demo binary and display are not needed; this
# measures CPU work only.
#
set -euo pipefail

cd "$(dirname "$0")/.."

NODES="${1:-100000}"

mkdir -p artifacts/bench
LOG="artifacts/bench/scene_${NODES}.log"

echo "==> $NODES hierarchy nodes through the native graph and the Rust reference, release mode"
cargo run -q --release -p mikan-scene --example bench_scene -- "$NODES" | tee "$LOG"

echo
echo "reminder: the native graph is expected to be markedly slower than the Rust"
echo "reference, which gets topology for free; the graph's value is its native"
echo "ownership and ABI, not raw speed. Quote the machine when quoting the numbers."
