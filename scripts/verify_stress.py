#!/usr/bin/env python3
"""Verify a `demo2d --scene stress` capture.

The stress scene draws N sprites of the `player` frame from one texture over
the whole viewport. The shell script checks the render stats (one batch, one
draw call); this script checks the frame itself:

* a large fraction of pixels are the player tile's warm colours, so the
  sprites were really rasterized with their texture, not merely counted, and
* the exact base colour appears somewhere, so no blend is faking the warm
  coverage.

The warm-pixel predicate is shared with `verify_camera.py`. Used by
`scripts/verify_stress.sh`.

Usage:
    python3 scripts/verify_stress.py FRAME.ppm [--min-warm FRACTION]
        [--min-covered FRACTION]
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from ppm import pixel, read_ppm
from verify_camera import warm_bbox

PLAYER_BASE = (253, 196, 108)


def parse_args(argv):
    if len(argv) < 2:
        raise SystemExit(
            f"usage: {argv[0]} FRAME.ppm [--min-warm FRACTION] [--min-covered FRACTION]"
        )
    path = argv[1]
    min_warm = 0.15
    min_covered = 0.20
    rest = argv[2:]
    while rest:
        option = rest.pop(0)
        if option not in ("--min-warm", "--min-covered"):
            raise SystemExit(f"error: unknown option {option!r}")
        if not rest:
            raise SystemExit(f"error: missing value for {option}")
        try:
            value = float(rest.pop(0))
        except ValueError:
            raise SystemExit(f"error: {option} expects a number")
        if option == "--min-warm":
            min_warm = value
        else:
            min_covered = value
    return path, min_warm, min_covered


def main(argv):
    path, min_warm, min_covered = parse_args(argv)
    width, height, payload = read_ppm(path)
    if width < 64 or height < 64:
        raise SystemExit(f"error: implausible capture size {width}x{height}")

    clear = pixel(payload, width, 2, 2)
    warm = 0
    covered = 0
    exact_base = 0
    for y in range(height):
        for x in range(width):
            value = pixel(payload, width, x, y)
            if value != clear:
                covered += 1
            if all(abs(a - b) <= 2 for a, b in zip(value, PLAYER_BASE)):
                exact_base += 1
            r, g, b = value
            if r > g + 20 and g > b + 20 and r > 120:
                warm += 1
    total = width * height
    warm_fraction = warm / total
    covered_fraction = covered / total
    print(f"capture: {width}x{height}")
    print(f"clear colour: {clear}")
    print(f"warm pixels: {warm}/{total} = {warm_fraction:.3f} (min {min_warm:.2f})")
    print(
        f"non-clear pixels: {covered}/{total} = {covered_fraction:.3f} "
        f"(min {min_covered:.2f})"
    )
    print(f"exact player base pixels: {exact_base}")

    if warm_fraction < min_warm:
        raise SystemExit(
            f"error: only {warm_fraction:.3f} of pixels are warm; the sprites are "
            "not textured, not drawn, or not covering the viewport"
        )
    if covered_fraction < min_covered:
        raise SystemExit(
            f"error: only {covered_fraction:.3f} of pixels are non-clear; the "
            "frame is mostly empty"
        )
    if exact_base == 0:
        raise SystemExit("error: no pixel matches the player base colour exactly")
    print("stress capture verified")


if __name__ == "__main__":
    main(sys.argv)
