#!/usr/bin/env python3
"""Verify a frame captured from the `--scene hierarchy` demonstration.

Three modes, one per documented behavior of the scene:

- ``layout``: no input at all, so the initial angles are exact. Samples the
  sun, the first planet, and the moon at their documented pixel positions.
- ``orbit``: a spin key was held, so the sun turned and its descendants
  orbited. Finds the moon's star pixels, and checks that their centroid is
  still exactly one orbit radius from the sun (rigid rotation) while its angle
  has changed.
- ``despawn``: `F` was pressed, so the moon's star pixels are gone while the
  sun and the planets remain.

Usage:
    python3 scripts/verify_scene.py MODE FRAME.ppm
"""

import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from ppm import pixel, read_ppm

# Documented world positions, in physical pixels for a 960x600 window.
SUN_CENTER = (480, 300)
PLANET_CENTER = (640, 300)
MOON_CENTER = (720, 300)
# orbit radius (160) + moon orbit radius (80).
MOON_DISTANCE = 240.0


def warm(rgb):
    """The sun's atlas frame: red above green above blue, all bright."""
    r, g, b = rgb
    return r > 200 and r > g > b


def blue(rgb):
    """A planet's marker frame: blue clearly dominant and bright."""
    r, g, b = rgb
    return b > 200 and b > r and g > 150


def star(rgb):
    """The moon's star texture: bright yellow, clearly not the sun's tile."""
    r, g, b = rgb
    return r > 200 and g > 200 and b < 170


def require(condition, message):
    if not condition:
        raise SystemExit(f"error: {message}")


def moon_pixels(payload, width, height):
    """Bright star pixels within the moon's orbit annulus around the sun."""
    found = []
    for y in range(height):
        for x in range(width):
            distance = math.hypot(x - SUN_CENTER[0], y - SUN_CENTER[1])
            if MOON_DISTANCE - 40 <= distance <= MOON_DISTANCE + 40:
                if star(pixel(payload, width, x, y)):
                    found.append((x, y))
    return found


def check_layout(payload, width):
    sun = pixel(payload, width, *SUN_CENTER)
    planet = pixel(payload, width, *PLANET_CENTER)
    moon = pixel(payload, width, *MOON_CENTER)
    print(f"    sun {SUN_CENTER}: {sun}")
    print(f"    planet {PLANET_CENTER}: {planet}")
    print(f"    moon {MOON_CENTER}: {moon}")
    require(warm(sun), f"the sun at {SUN_CENTER} is {sun}, expected a warm tile")
    require(blue(planet), f"the first planet at {PLANET_CENTER} is {planet}, expected blue")
    require(star(moon), f"the moon at {MOON_CENTER} is {moon}, expected the yellow star")


def check_orbit(payload, width, height):
    found = moon_pixels(payload, width, height)
    require(len(found) > 0, "no moon pixels found in orbit; the spin did not move it")
    cx = sum(x for x, _ in found) / len(found)
    cy = sum(y for _, y in found) / len(found)
    distance = math.hypot(cx - SUN_CENTER[0], cy - SUN_CENTER[1])
    angle = math.degrees(math.atan2(cy - SUN_CENTER[1], cx - SUN_CENTER[0]))
    print(
        f"    moon centroid ({cx:.1f}, {cy:.1f}): distance {distance:.1f} px, "
        f"angle {angle:.1f} deg from the sun"
    )
    require(
        abs(distance - MOON_DISTANCE) <= 12,
        f"the moon is {distance:.1f} px from the sun; rigid rotation keeps it at "
        f"{MOON_DISTANCE:.0f}",
    )
    require(abs(angle) > 5.0, f"the moon's angle is only {angle:.1f} deg; the sun did not spin")


def check_despawn(payload, width, height):
    found = moon_pixels(payload, width, height)
    require(
        len(found) == 0,
        f"the moon still has {len(found)} pixels after F; the despawn did not apply",
    )
    sun = pixel(payload, width, *SUN_CENTER)
    require(warm(sun), f"the sun is {sun} after the moon was removed")
    planets = sum(
        1
        for y in range(height)
        for x in range(width)
        if blue(pixel(payload, width, x, y))
    )
    print(f"    star pixels after F: 0; planet pixels: {planets}")
    require(planets > 0, "the planets disappeared with the moon")


def main(argv):
    if len(argv) != 3:
        raise SystemExit(f"usage: {argv[0]} MODE FRAME.ppm")
    mode, path = argv[1], argv[2]
    width, height, payload = read_ppm(path)
    if width < 64 or height < 64:
        raise SystemExit(f"error: implausible capture size {width}x{height}")

    if mode == "layout":
        check_layout(payload, width)
    elif mode == "orbit":
        check_orbit(payload, width, height)
    elif mode == "despawn":
        check_despawn(payload, width, height)
    else:
        raise SystemExit(f"error: unknown mode {mode!r}; expected layout, orbit, or despawn")
    print(f"scene {mode} verified")


if __name__ == "__main__":
    main(sys.argv)
