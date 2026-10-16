#!/usr/bin/env bash
#
# Verifies asset handling end to end: the committed PNGs match their
# generator, and a missing, corrupt, or incomplete asset set stops the demo
# with a readable error instead of rendering a broken frame.
#
# Checks:
#   1. `scripts/generate_assets.py --check` passes: the committed atlas,
#      sheet, and star are exactly what the generator produces.
#   2. `--assets` pointing at a missing directory fails with "could not read".
#   3. A sheet whose texture is not a PNG fails with "could not decode PNG
#      data".
#   4. A sheet without the "player" frame fails naming the frame.
#
# Requires: a display (Xvfb is fine), python3, and a release build of the demo:
#
#     xvfb-run -a bash scripts/verify_assets.sh
#
set -euo pipefail

cd "$(dirname "$0")/.."

DEMO_BIN="${DEMO_BIN:-./target/release/demo2d}"
TIMEOUT_SECS="${TIMEOUT_SECS:-15}"

# shellcheck source=scripts/lib.sh
source scripts/lib.sh
check_demo_environment

# Runs the demo expecting failure; requires a substring in its output.
expect_failure() {
    local description=$1 expected=$2
    shift 2
    local output
    if output="$("$DEMO_BIN" --frames 1 --timeout "$TIMEOUT_SECS" "$@" 2>&1)"; then
        echo "error: $description: the demo exited successfully" >&2
        exit 1
    fi
    if ! grep -qF "$expected" <<<"$output"; then
        echo "error: $description: output does not mention \"$expected\":" >&2
        echo "$output" >&2
        exit 1
    fi
    echo "    PASS: $description"
}

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

echo "==> assets: the committed files match scripts/generate_assets.py"
python3 scripts/generate_assets.py --check
echo "    PASS: generator check"

echo "==> assets: malformed asset sets fail with readable errors"
expect_failure "missing asset directory" "could not read" \
    --assets "$fixture/absent"

mkdir -p "$fixture/corrupt"
cp games/demo2d/assets/atlas.json "$fixture/corrupt/"
printf 'this file is not a PNG' >"$fixture/corrupt/atlas.png"
expect_failure "corrupt PNG texture" "could not decode PNG data" \
    --assets "$fixture/corrupt"

mkdir -p "$fixture/missing-frame"
cp games/demo2d/assets/atlas.png games/demo2d/assets/star.png \
    "$fixture/missing-frame/"
python3 - "$fixture/missing-frame/atlas.json" <<'PY'
import json
import sys

# The committed sheet minus the frame the scenes require, to prove the check
# names the frame that is missing.
with open("games/demo2d/assets/atlas.json", encoding="utf-8") as handle:
    document = json.load(handle)
del document["frames"]["player"]
with open(sys.argv[1], "w", encoding="utf-8") as handle:
    json.dump(document, handle, indent=2)
PY
expect_failure "sheet without the player frame" 'has no frame "player"' \
    --assets "$fixture/missing-frame"

echo
echo "asset verification passed"
