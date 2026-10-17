#!/usr/bin/env bash
#
# Measures the mixer (the C++ runtime through mikan-audio's safe API) against
# the 48 kHz real-time budget: release mode, warm-up, repeated runs, and the
# deterministic offline backend, so the numbers do not depend on a sound card.
# See crates/audio/examples/bench_audio.rs for the method, and docs/audio.md
# for how to read the numbers.
#
# The result is written to artifacts/bench/audio.log.
#
# Usage:
#     bash scripts/bench_audio.sh [BLOCKS]
#
# Defaults: BLOCKS=4096 (about 87 s of audio per run). A display and an audio
# device are not needed; this measures CPU work only.
#
set -euo pipefail

cd "$(dirname "$0")/.."

BLOCKS="${1:-4096}"

mkdir -p artifacts/bench
LOG="artifacts/bench/audio_${BLOCKS}.log"

echo "==> $BLOCKS blocks through the mixer, release mode"
cargo run -q --release -p mikan-audio --example bench_audio -- "$BLOCKS" | tee "$LOG"

echo
echo "reminder: the ns/frame row is the number the audio thread must keep under"
echo "20833 ns (one frame at 48 kHz) once per voice and per block; the 64-voice"
echo "row is the worst case the pool allows. Quote the machine with the numbers."
