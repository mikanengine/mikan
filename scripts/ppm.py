#!/usr/bin/env python3
"""Shared helpers for reading the binary PPM (P6) frames demo2d captures.

Both ``verify_capture.py`` (whole-frame smoke check) and
``verify_camera.py`` (camera geometry check) parse the same files, so the
format handling lives here once. Failures raise ``SystemExit`` with a readable
message, which is what a command-line tool should print.
"""


def read_ppm(path):
    """Parses a binary PPM file, returning ``(width, height, payload)``."""
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
    """Returns the ``(r, g, b)`` tuple at ``(x, y)`` of a decoded payload."""
    index = (y * width + x) * 3
    return tuple(payload[index : index + 3])
