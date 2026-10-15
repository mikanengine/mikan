#!/usr/bin/env python3
"""Verify a frame captured by `demo2d --capture`.

Checks that the file is a well-formed binary PPM (P6), that its payload size
matches its declared dimensions, and that the frame actually contains the demo's
scene: an orange player square over a dark background.

Used by CI after the headless smoke test. Exits non-zero with a readable message
on any failure.

Usage:
    python3 scripts/verify_capture.py artifacts/frame.ppm
"""

import sys


def read_ppm(path):
    """Parses a binary PPM file, returning (width, height, payload)."""
    with open(path, "rb") as handle:
        data = handle.read()
    if not data.startswith(b"P6\n"):
        raise SystemExit(f"error: {path} is not a binary PPM (P6) file")
    _, rest = data.split(b"\n", 1)
    dimensions, rest = rest.split(b"\n", 1)
    try:
        width, height = (int(value) for value in dimensions.split())
    except ValueError:
        raise SystemExit(f"error: unreadable PPM dimensions {dimensions!r}")
    maximum, payload = rest.split(b"\n", 1)
    if maximum != b"255":
        raise SystemExit(f"error: expected a maximum value of 255, found {maximum!r}")
    expected = width * height * 3
    if len(payload) != expected:
        raise SystemExit(
            f"error: expected {expected} payload bytes for {width}x{height}, "
            f"found {len(payload)}"
        )
    return width, height, payload


def pixel(payload, width, x, y):
    index = (y * width + x) * 3
    return tuple(payload[index : index + 3])


def main(argv):
    if len(argv) != 2:
        raise SystemExit(f"usage: {argv[0]} FRAME.ppm")
    width, height, payload = read_ppm(argv[1])
    if width < 64 or height < 64:
        raise SystemExit(f"error: implausible capture size {width}x{height}")

    background = pixel(payload, width, 2, 2)
    center = pixel(payload, width, width // 2, height // 2)
    print(f"capture: {width}x{height}")
    print(f"background corner: {background}")
    print(f"center: {center}")

    if background == center:
        raise SystemExit("error: the center pixel matches the background; the frame is empty")
    if not center[0] > center[2]:
        raise SystemExit(
            f"error: unexpected center color {center}; expected the orange player square"
        )
    print("capture verified")


if __name__ == "__main__":
    main(sys.argv)
