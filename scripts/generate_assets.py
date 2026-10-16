#!/usr/bin/env python3
"""Generates demo2d's PNG assets and sprite-sheet metadata.

The demo loads real files at runtime, so the files it loads are committed to
the repository and produced by this script. Everything is deterministic: fixed
colors and geometry, fixed PNG writing (filter 0, zlib level 9), fixed JSON
formatting. ``--check`` regenerates in memory and compares against the files on
disk, which is how CI catches an asset that was edited by hand or an image that
no longer matches the script.

Assets written under ``games/demo2d/assets``:

* ``atlas.png`` — three 32x32 tiles side by side (96x32): ``player`` (orange,
  dark border, light diagonal), ``marker`` (blue, same structure), and
  ``checker`` (8-pixel black/white squares, for the sampler comparison).
* ``atlas.json`` — the sprite-sheet document naming those frames, in the
  Aseprite subset ``mikan-assets`` reads (see docs/sprite-sheets.md).
* ``star.png`` — a 24x24 five-pointed yellow star with anti-aliased edges and
  fully transparent corners, for proving PNG alpha.

Colors are specified as linear sRGB components, the units the engine speaks;
the PNG stores the sRGB-encoded 8-bit values a display would show, which is
what ``TextureEncoding::Srgb`` expects to decode.

Usage:
    python3 scripts/generate_assets.py          # write the files
    python3 scripts/generate_assets.py --check  # fail if the files differ
"""

import json
import math
import struct
import sys
import zlib
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parent.parent
ASSETS = REPOSITORY / "games" / "demo2d" / "assets"

TILE = 32
ATLAS_WIDTH = TILE * 3
ATLAS_HEIGHT = TILE
BORDER = 3
DIAGONAL = 16

PLAYER_PALETTE = {
    "base": (0.98, 0.55, 0.15),
    "border": (0.45, 0.22, 0.05),
    "accent": (1.0, 0.85, 0.55),
}
MARKER_PALETTE = {
    "base": (0.35, 0.75, 0.95),
    "border": (0.10, 0.30, 0.50),
    "accent": (0.85, 0.95, 1.0),
}
STAR_COLOR = (1.0, 0.90, 0.20)

STAR_SIZE = 24
STAR_OUTER_RADIUS = 11.0
STAR_INNER_RADIUS = 4.4
# 4x4 samples per pixel turn star edges into partial alpha instead of a
# staircase; corners stay fully transparent because the star does not reach
# them.
STAR_SUPERSAMPLING = 4


def srgb_byte(linear):
    """The 8-bit sRGB-encoded value a display shows for a linear component."""
    if linear <= 0.0031308:
        encoded = linear * 12.92
    else:
        encoded = 1.055 * (linear ** (1.0 / 2.4)) - 0.055
    return round(min(max(encoded, 0.0), 1.0) * 255)


def tile_pixel(palette, x, y):
    on_border = (
        x < BORDER
        or y < BORDER
        or x >= TILE - BORDER
        or y >= TILE - BORDER
    )
    on_diagonal = x + y < DIAGONAL
    if on_border:
        color = palette["border"]
    elif on_diagonal:
        color = palette["accent"]
    else:
        color = palette["base"]
    return tuple(srgb_byte(component) for component in color) + (255,)


def checker_pixel(x, y):
    white = ((x // 8) + (y // 8)) % 2 == 0
    value = 255 if white else 0
    return (value, value, value, 255)


def star_pixel(x, y):
    """A five-pointed star, supersampled for anti-aliased edges."""
    center = STAR_SIZE / 2.0
    coverage = 0
    samples = STAR_SUPERSAMPLING
    for sy in range(samples):
        for sx in range(samples):
            px = x + (sx + 0.5) / samples
            py = y + (sy + 0.5) / samples
            if inside_star(px - center, py - center):
                coverage += 1
    if coverage == 0:
        return (0, 0, 0, 0)
    alpha = round(255 * coverage / (samples * samples))
    return tuple(srgb_byte(component) for component in STAR_COLOR) + (alpha,)


def star_vertices():
    """Ten alternating outer/inner vertices of the star, top point first."""
    vertices = []
    for index in range(10):
        radius = STAR_OUTER_RADIUS if index % 2 == 0 else STAR_INNER_RADIUS
        angle = -math.pi / 2 + index * math.pi / 5
        vertices.append((radius * math.cos(angle), radius * math.sin(angle)))
    return vertices


def inside_star(x, y):
    """Even-odd point-in-polygon test against :func:`star_vertices`."""
    vertices = star_vertices()
    inside = False
    count = len(vertices)
    for index in range(count):
        x1, y1 = vertices[index]
        x2, y2 = vertices[(index + 1) % count]
        if (y1 > y) != (y2 > y):
            cross_x = x1 + (y - y1) * (x2 - x1) / (y2 - y1)
            if x < cross_x:
                inside = not inside
    return inside


def raw_scanlines(width, height, pixel):
    """Filter-byte-0 rows of the pixels `pixel(x, y)` returns."""
    raw = bytearray()
    for y in range(height):
        raw.append(0)
        for x in range(width):
            raw.extend(pixel(x, y))
    return raw


def png_bytes(width, height, pixel):
    """A minimal RGBA8 PNG: filter 0 on every row, one IDAT."""
    raw = raw_scanlines(width, height, pixel)

    def chunk(kind, data):
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
        )

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def atlas_pixels(x, y):
    if x < TILE:
        return tile_pixel(PLAYER_PALETTE, x, y)
    if x < 2 * TILE:
        return tile_pixel(MARKER_PALETTE, x - TILE, y)
    return checker_pixel(x - 2 * TILE, y)


def atlas_json():
    document = {
        "frames": {
            "player": {
                "frame": {"x": 0, "y": 0, "w": TILE, "h": TILE},
                "duration": 100,
            },
            "marker": {
                "frame": {"x": TILE, "y": 0, "w": TILE, "h": TILE},
                "duration": 100,
            },
            "checker": {
                "frame": {"x": 2 * TILE, "y": 0, "w": TILE, "h": TILE},
                "duration": 100,
            },
        },
        "meta": {
            "image": "atlas.png",
            "size": {"w": ATLAS_WIDTH, "h": ATLAS_HEIGHT},
            "format": "RGBA8888",
            "scale": "1",
        },
    }
    return (json.dumps(document, indent=2) + "\n").encode()


def expected_files():
    return {
        "atlas.png": png_bytes(ATLAS_WIDTH, ATLAS_HEIGHT, atlas_pixels),
        "star.png": png_bytes(STAR_SIZE, STAR_SIZE, star_pixel),
        "atlas.json": atlas_json(),
    }


def read_png_raw(path):
    """Decodes a PNG written by :func:`png_bytes` back to its raw scanlines."""
    data = path.read_bytes()
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        raise SystemExit(f"error: {path} is not a PNG file")
    offset = 8
    width = height = None
    raw = bytearray()
    while offset < len(data):
        (length,) = struct.unpack(">I", data[offset : offset + 4])
        kind = data[offset + 4 : offset + 8]
        payload = data[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            width, height = struct.unpack(">II", payload[:8])
            if payload[8:] != bytes([8, 6, 0, 0, 0]):
                raise SystemExit(f"error: {path} is not the expected 8-bit RGBA format")
        elif kind == b"IDAT":
            raw.extend(payload)
        elif kind == b"IEND":
            break
    if width is None or height is None:
        raise SystemExit(f"error: {path} has no header")
    return width, height, bytearray(zlib.decompress(bytes(raw)))


def check():
    failures = []
    pngs = {
        "atlas.png": (ATLAS_WIDTH, ATLAS_HEIGHT, atlas_pixels),
        "star.png": (STAR_SIZE, STAR_SIZE, star_pixel),
    }
    json_expectations = {"atlas.json": atlas_json()}

    for name, (width, height, pixel) in pngs.items():
        path = ASSETS / name
        if not path.exists():
            failures.append(f"{path} is missing")
            continue
        actual_width, actual_height, raw = read_png_raw(path)
        if (actual_width, actual_height) != (width, height):
            failures.append(
                f"{path} is {actual_width}x{actual_height}, expected {width}x{height}"
            )
        elif raw != raw_scanlines(width, height, pixel):
            failures.append(f"{path} does not match `generate_assets.py`")
        else:
            print(f"ok: {path} matches the generator")

    for name, expected in json_expectations.items():
        path = ASSETS / name
        if not path.exists():
            failures.append(f"{path} is missing")
        elif path.read_bytes() != expected:
            failures.append(f"{path} does not match `generate_assets.py`")
        else:
            print(f"ok: {path} matches the generator")

    if failures:
        for failure in failures:
            print(f"error: {failure}", file=sys.stderr)
        print("run `python3 scripts/generate_assets.py` to refresh the assets", file=sys.stderr)
        return 1
    return 0


def write():
    ASSETS.mkdir(parents=True, exist_ok=True)
    for name, contents in expected_files().items():
        path = ASSETS / name
        path.write_bytes(contents)
        print(f"wrote {path} ({len(contents)} bytes)")
    return 0


def main(argv):
    if argv[1:] == ["--check"]:
        return check()
    if argv[1:]:
        raise SystemExit(f"usage: {argv[0]} [--check]")
    return write()


if __name__ == "__main__":
    sys.exit(main(sys.argv))
