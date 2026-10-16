#!/usr/bin/env python3
"""Summarize the demo logs `scripts/bench_sprites.sh` recorded.

Each log contains the demo's own lines:

* `scene stress | sampler ... | window WxH` — the window the run used;
* `stress: N sprites, CxR cells` — the sprite count;
* `render: ... | sprites N | batches B | draw calls D (...)` — what the last
  presented frame submitted;
* `frames: F | mean .. ms | p50 .. ms | p95 .. ms | max .. ms | mean X fps` —
  the measured frame times.

The script prints one table row per log, sorted by window area then sprite
count, and nothing else. Numbers come from a software rasterizer unless the
logs were produced on real hardware; they measure this pipeline, not GPUs in
general.

Usage:
    python3 scripts/bench_report.py LOG [LOG ...]
"""

import os
import re
import sys

PATTERN = re.compile(
    r"window (\d+)x(\d+).*?"
    r"stress: (\d+) sprites.*?"
    r"render: .*? sprites (\d+) \| batches (\d+) \| draw calls (\d+).*?"
    r"frames: (\d+) \| mean ([\d.]+) ms \| p50 ([\d.]+) ms \| p95 ([\d.]+) ms "
    r"\| max ([\d.]+) ms \| mean ([\d.]+) fps",
    re.DOTALL,
)

HEADER = (
    "| window | sprites | batches | draws | frames "
    "| mean ms | p50 ms | p95 ms | max ms | fps |"
)
SEPARATOR = "|---|---|---|---|---|---|---|---|---|---|"


def parse(path):
    with open(path, encoding="utf-8", errors="replace") as handle:
        text = handle.read()
    match = PATTERN.search(text)
    if not match:
        raise SystemExit(f"error: {path} is not a bench log; see scripts/bench_sprites.sh")
    (
        width,
        height,
        requested,
        sprites,
        batches,
        draws,
        frames,
        mean,
        p50,
        p95,
        maximum,
        fps,
    ) = match.groups()
    if sprites != requested:
        raise SystemExit(
            f"error: {path} reports {sprites} sprites but the scene was built for "
            f"{requested}; the log is inconsistent"
        )
    return {
        "width": int(width),
        "height": int(height),
        "sprites": int(sprites),
        "batches": int(batches),
        "draws": int(draws),
        "frames": int(frames),
        "mean": float(mean),
        "p50": float(p50),
        "p95": float(p95),
        "max": float(maximum),
        "fps": float(fps),
    }


def main(argv):
    if len(argv) < 2:
        raise SystemExit(f"usage: {argv[0]} LOG [LOG ...]")
    runs = [parse(path) for path in argv[1:]]
    runs.sort(key=lambda run: (run["width"] * run["height"], run["sprites"]))

    print(HEADER)
    print(SEPARATOR)
    for run in runs:
        print(
            f"| {run['width']}x{run['height']} | {run['sprites']} | {run['batches']} "
            f"| {run['draws']} | {run['frames']} | {run['mean']:.2f} "
            f"| {run['p50']:.2f} | {run['p95']:.2f} | {run['max']:.2f} | {run['fps']:.1f} |"
        )


if __name__ == "__main__":
    main(sys.argv)
