#!/usr/bin/env python3
"""Verify a camera capture from `demo2d --camera follow`.

Checks the demo's player square in a captured frame:

* the square is fully inside the frame,
* it is *centred* on screen, which is what the follow camera promises, and
* it has the expected on-screen size, which is how `--zoom` is verified.

Detection is colour-based: the player tile (base, border, and highlight) is
warm — red above green above blue — while the background and the marker tile
are not, so the warm pixels are exactly the player square.

Used by `scripts/verify_camera.sh`. Exits non-zero with a readable message on
any failure.

Usage:
    python3 scripts/verify_camera.py FRAME.ppm [--size PX] [--size-tolerance PX]
                                             [--center-tolerance PX]
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from ppm import read_ppm


def warm_bbox(payload, width, height):
    """Bounding box, pixel count, and centroid of the warm (orange) pixels."""
    min_x, min_y = width, height
    max_x, max_y = -1, -1
    count = 0
    sum_x = sum_y = 0
    for y in range(height):
        for x in range(width):
            index = (y * width + x) * 3
            r, g, b = payload[index], payload[index + 1], payload[index + 2]
            if r > g + 20 and g > b + 20 and r > 120:
                count += 1
                sum_x += x
                sum_y += y
                min_x, min_y = min(min_x, x), min(min_y, y)
                max_x, max_y = max(max_x, x), max(max_y, y)
    if count == 0:
        return None
    centroid = (sum_x / count, sum_y / count)
    return (min_x, min_y, max_x, max_y), count, centroid


def parse_args(argv):
    if len(argv) < 2:
        raise SystemExit(
            f"usage: {argv[0]} FRAME.ppm [--size PX] [--size-tolerance PX] "
            "[--center-tolerance PX]"
        )
    path = argv[1]
    size = None
    size_tolerance = 8.0
    center_tolerance = 8.0
    rest = argv[2:]
    while rest:
        option = rest.pop(0)
        if option not in ("--size", "--size-tolerance", "--center-tolerance"):
            raise SystemExit(f"error: unknown option {option!r}")
        if not rest:
            raise SystemExit(f"error: missing value for {option}")
        try:
            value = float(rest.pop(0))
        except ValueError:
            raise SystemExit(f"error: {option} expects a number")
        if option == "--size":
            size = value
        elif option == "--size-tolerance":
            size_tolerance = value
        else:
            center_tolerance = value
    return path, size, size_tolerance, center_tolerance


def main(argv):
    path, expected_size, size_tolerance, center_tolerance = parse_args(argv)
    width, height, payload = read_ppm(path)
    if width < 64 or height < 64:
        raise SystemExit(f"error: implausible capture size {width}x{height}")

    box = warm_bbox(payload, width, height)
    if box is None:
        raise SystemExit("error: no player square (warm pixels) found in the frame")
    (min_x, min_y, max_x, max_y), count, (cx, cy) = box
    square_width = max_x - min_x + 1
    square_height = max_y - min_y + 1
    center_x, center_y = width / 2, height / 2
    print(f"capture: {width}x{height}")
    print(
        f"player square: {square_width}x{square_height} px at "
        f"[{min_x}..{max_x}, {min_y}..{max_y}], centroid ({cx:.1f}, {cy:.1f}), "
        f"{count} pixels"
    )

    if min_x == 0 or min_y == 0 or max_x == width - 1 or max_y == height - 1:
        raise SystemExit("error: the player square touches the frame edge; the camera is off")

    if abs(cx - center_x) > center_tolerance or abs(cy - center_y) > center_tolerance:
        raise SystemExit(
            f"error: the square is centred at ({cx:.1f}, {cy:.1f}), not at the "
            f"capture centre ({center_x:.0f}, {center_y:.0f}); the camera did not "
            "follow the player"
        )

    if expected_size is not None:
        for label, measured in (
            ("width", square_width),
            ("height", square_height),
        ):
            if abs(measured - expected_size) > size_tolerance:
                raise SystemExit(
                    f"error: the square's {label} is {measured} px, expected "
                    f"{expected_size:.0f} px (+-{size_tolerance:.0f}); zoom is wrong"
                )
        print(f"size check: {square_width}x{square_height} px, expected {expected_size:.0f} px")

    print("camera capture verified")


if __name__ == "__main__":
    main(sys.argv)
