#!/usr/bin/env python3
"""Verify a `demo2d --scene showcase` capture.

The showcase scene (`games/demo2d/src/scene.rs`) draws eight sprites at fixed
world positions, each chosen so that one Phase 3 feature is visible at a known
pixel. This script checks them all against the frame:

* plain tile: the player texture's base colour arrives unchanged (PNG upload,
  sRGB round trip);
* rotated tile: pixels outside the unrotated square's box but inside the
  45-degree diamond show the tile, while the unrotated box's corner is clear
  (rotation reaches outside the axis-aligned quad);
* tinted tile: the tint multiplies the sampled colour;
* alpha tile: a 0.4-alpha tint blends the tile over the clear color;
* checker pair: pinned linear sampling produces intermediate values across
  the scanline, pinned nearest sampling only produces the source black/white;
* star: a yellow centre over its backdrop, and transparent corners where the
  backdrop shows through the sprite (PNG alpha).

Expected colours are the values this software-rendered pipeline produces
(sRGB surface, linear tinting; see docs/status). Tolerances absorb the +-1
rounding of capture and GPU sampling.

Usage:
    python3 scripts/verify_showcase.py FRAME.ppm
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from ppm import pixel, read_ppm

# Scene constants mirrored from games/demo2d/src/scene.rs.
ROW1_Y = 140
X_PLAIN = 140
X_ROTATED = 340
X_TINTED = 540
X_ALPHA = 740
ROW2_Y = 330
X_LINEAR = 340
X_NEAREST = 620
CHECKER_SIZE = 176
STAR_CENTER = (480, 500)
STAR_SIZE = 96

# Measured pipeline output (tolerance applied below).
CLEAR = (63, 69, 80)
PLAYER_BASE = (253, 196, 108)
PLAYER_BORDER = (179, 129, 63)
TINTED = (136, 156, 108)
ALPHA_BLENDED = (174, 138, 93)
BACKDROP = (80, 97, 149)
STAR_YELLOW = (255, 243, 124)

TOLERANCE = 6


class Checker:
    """Collects failures so one run reports everything that is wrong."""

    def __init__(self):
        self.failures = []

    def near(self, name, actual, expected, tolerance=TOLERANCE):
        ok = all(
            abs(a - e) <= tolerance for a, e in zip(actual, expected)
        )
        print(f"  {name}: {actual} (expected {expected} +-{tolerance})")
        if not ok:
            self.failures.append(
                f"{name} is {actual}, expected {expected} +-{tolerance}"
            )

    def condition(self, name, ok, detail):
        print(f"  {name}: {detail}")
        if not ok:
            self.failures.append(f"{name}: {detail}")


def grayscale(values):
    """True when every sampled pixel is gray (r == g == b within rounding)."""
    return all(
        abs(r - g) <= 2 and abs(g - b) <= 2 for r, g, b in values
    )


def intermediate_count(values, low=16, high=239):
    """Pixels strictly between black and white: linear filtering's signature."""
    return sum(
        1
        for r, g, b in values
        if low < r < high and low < g < high and low < b < high
    )


def scanline(payload, width, y, x0, x1):
    return [pixel(payload, width, x, y) for x in range(x0, x1 + 1)]


def column(payload, width, x, y0, y1):
    return [pixel(payload, width, x, y) for y in range(y0, y1 + 1)]


def main(argv):
    if len(argv) != 2:
        raise SystemExit(f"usage: {argv[0]} FRAME.ppm")
    width, height, payload = read_ppm(argv[1])
    if width < 64 or height < 64:
        raise SystemExit(f"error: implausible capture size {width}x{height}")

    def px(x, y):
        return pixel(payload, width, x, y)

    checker = Checker()
    print(f"capture: {width}x{height}")

    print("plain tile (texture upload and sRGB round trip):")
    checker.near("clear corner", px(2, 2), CLEAR)
    checker.near("centre", px(X_PLAIN, ROW1_Y), PLAYER_BASE)

    print("rotated tile (45 degrees clockwise):")
    # The unrotated square at x=340 spans [292..388] x [92..188]; (300, 100)
    # is inside that box but outside the rotated diamond, so seeing the clear
    # color there proves the quad rotated.
    checker.near("unrotated box corner is clear", px(300, 100), CLEAR)
    # The diamond's tip reaches y = 140 - 48*sqrt(2) ~= 72; an unrotated
    # square would never paint anything above y = 92.
    checker.near("diamond tip above the box", px(X_ROTATED, 76), PLAYER_BORDER)

    print("tinted tile (linear tint of the sampled texel):")
    checker.near("centre", px(X_TINTED, ROW1_Y), TINTED)

    print("alpha tile (0.4 alpha over the clear color):")
    checker.near("centre", px(X_ALPHA, ROW1_Y), ALPHA_BLENDED)

    print("checker pair (5.5x magnification, pinned samplers):")
    inset = 8
    linear_h = scanline(
        payload,
        width,
        ROW2_Y,
        X_LINEAR - CHECKER_SIZE // 2 + inset,
        X_LINEAR + CHECKER_SIZE // 2 - 1 - inset,
    )
    nearest_h = scanline(
        payload,
        width,
        ROW2_Y,
        X_NEAREST - CHECKER_SIZE // 2 + inset,
        X_NEAREST + CHECKER_SIZE // 2 - 1 - inset,
    )
    linear_v = column(
        payload,
        width,
        X_LINEAR,
        ROW2_Y - CHECKER_SIZE // 2 + inset,
        ROW2_Y + CHECKER_SIZE // 2 - 1 - inset,
    )
    nearest_v = column(
        payload,
        width,
        X_NEAREST,
        ROW2_Y - CHECKER_SIZE // 2 + inset,
        ROW2_Y + CHECKER_SIZE // 2 - 1 - inset,
    )
    for label, values in (
        ("linear horizontal", linear_h),
        ("linear vertical", linear_v),
    ):
        distinct = len(set(values))
        intermediate = intermediate_count(values)
        checker.condition(
            label,
            grayscale(values) and distinct >= 8 and intermediate >= 100,
            f"{len(values)} px, {distinct} distinct grays, {intermediate} intermediate",
        )
    for label, values in (
        ("nearest horizontal", nearest_h),
        ("nearest vertical", nearest_v),
    ):
        # Every pixel must be (near) black or (near) white; nearest sampling
        # never invents an intermediate value.
        ok = all(
            all(abs(channel - 0) <= 2 or abs(channel - 255) <= 2 for channel in value)
            for value in values
        )
        checker.condition(
            label,
            ok and grayscale(values),
            f"{len(values)} px, all in {{black, white}}",
        )

    print("star over its backdrop (PNG alpha and painter order):")
    star_half = STAR_SIZE // 2
    checker.near("yellow centre", px(*STAR_CENTER), STAR_YELLOW)
    checker.near(
        "transparent corner shows backdrop",
        px(STAR_CENTER[0] - star_half + 8, STAR_CENTER[1] - star_half + 8),
        BACKDROP,
    )
    checker.near(
        "concave edge shows backdrop",
        px(STAR_CENTER[0] - star_half + 8, STAR_CENTER[1]),
        BACKDROP,
    )

    if checker.failures:
        raise SystemExit("error: " + "; ".join(checker.failures))
    print("showcase capture verified")


if __name__ == "__main__":
    main(sys.argv)
