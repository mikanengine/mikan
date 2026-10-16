#!/usr/bin/env python3
"""Verify a capture from `demo2d --pixel-perfect`.

In pixel-perfect mode the demo derives an integer zoom from the virtual screen
size (the shell script checks the `integer zoom xN` log line) and snaps the
camera to the physical pixel grid. This script checks the visual consequence
in a captured frame:

* the player square has the expected physical size — `96` virtual pixels
  (`PLAYER_SIZE` in `games/demo2d/src/scene.rs`) times the zoom — and
* every edge of the square's bounding box lies on a multiple of the zoom,
  which only happens if the whole virtual-pixel grid is aligned to the
  physical grid: with a fractional camera position the quad's edges would
  land between pixels and the warm bounding box would grow or shrink by a
  pixel at some edges.

Detection is colour-based, reusing the player-square heuristic from
`verify_camera.py`. Used by `scripts/verify_pixel_perfect.sh`.

Usage:
    python3 scripts/verify_pixel_perfect.py FRAME.ppm --scale N [--size PX]
        [--size-tolerance PX] [--center-tolerance PX]
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from ppm import read_ppm
from verify_camera import warm_bbox


def parse_args(argv):
    if len(argv) < 2:
        raise SystemExit(
            f"usage: {argv[0]} FRAME.ppm --scale N [--size PX] "
            "[--size-tolerance PX] [--center-tolerance PX]"
        )
    path = argv[1]
    scale = None
    size = None
    size_tolerance = 1.0
    center_tolerance = 2.0
    rest = argv[2:]
    while rest:
        option = rest.pop(0)
        if option not in (
            "--scale",
            "--size",
            "--size-tolerance",
            "--center-tolerance",
        ):
            raise SystemExit(f"error: unknown option {option!r}")
        if not rest:
            raise SystemExit(f"error: missing value for {option}")
        try:
            value = float(rest.pop(0))
        except ValueError:
            raise SystemExit(f"error: {option} expects a number")
        if option == "--scale":
            if value < 1 or value != int(value):
                raise SystemExit(f"error: {option} expects an integer >= 1")
            scale = int(value)
        elif option == "--size":
            size = value
        elif option == "--size-tolerance":
            size_tolerance = value
        else:
            center_tolerance = value
    if scale is None:
        raise SystemExit("error: --scale is required (the logged integer zoom)")
    return path, scale, size, size_tolerance, center_tolerance


def main(argv):
    path, scale, expected_size, size_tolerance, center_tolerance = parse_args(argv)
    width, height, payload = read_ppm(path)
    if width < 64 or height < 64:
        raise SystemExit(f"error: implausible capture size {width}x{height}")

    box = warm_bbox(payload, width, height)
    if box is None:
        raise SystemExit("error: no player square (warm pixels) found in the frame")
    (min_x, min_y, max_x, max_y), _, _ = box
    square_width = max_x - min_x + 1
    square_height = max_y - min_y + 1
    print(f"capture: {width}x{height}, pixel-perfect scale {scale}")
    print(
        f"player square: {square_width}x{square_height} px at "
        f"[{min_x}..{max_x}, {min_y}..{max_y}]"
    )

    failures = []
    if min_x == 0 or min_y == 0 or max_x == width - 1 or max_y == height - 1:
        failures.append("the player square touches the frame edge; the camera is off")

    if expected_size is not None:
        for label, measured in (("width", square_width), ("height", square_height)):
            if abs(measured - expected_size) > size_tolerance:
                failures.append(
                    f"the square's {label} is {measured} px, expected "
                    f"{expected_size:.0f} px (+-{size_tolerance:.0f})"
                )

    # Edges on the physical grid: an edge is [min, max], and "on the grid"
    # means the edge's boundary coordinate (min, or max + 1) is a multiple of
    # the zoom. A square of the right size but shifted half a pixel would
    # fail this while still passing the size check.
    aligned = True
    for label, edge in (
        ("left", min_x),
        ("top", min_y),
        ("right+1", max_x + 1),
        ("bottom+1", max_y + 1),
    ):
        if edge % scale != 0:
            aligned = False
            failures.append(
                f"the square's {label} edge is at {edge}, not a multiple of the "
                f"zoom {scale}; the pixel grid is not aligned"
            )
    if aligned:
        print(f"grid alignment: all edges on multiples of {scale}")

    center = ((min_x + max_x + 1) / 2, (min_y + max_y + 1) / 2)
    if (
        abs(center[0] - width / 2) > center_tolerance
        or abs(center[1] - height / 2) > center_tolerance
    ):
        failures.append(
            f"the square's centre is {center}, not the capture centre "
            f"({width / 2:.0f}, {height / 2:.0f}) +-{center_tolerance:.0f}"
        )
    else:
        print(f"centre: ({center[0]:.1f}, {center[1]:.1f})")

    if failures:
        raise SystemExit("error: " + "; ".join(failures))
    print("pixel-perfect capture verified")


if __name__ == "__main__":
    main(sys.argv)
