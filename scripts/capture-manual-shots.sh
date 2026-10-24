#!/usr/bin/env bash
# Render the pictures of both user manuals (docs/manual and docs/guide) from
# the running app, at one fixed 1440 × 900 window, into
# docs/manual/src/images (the Quick Guide's guide-* pictures then move to
# docs/guide/src/images).
#
#   ./scripts/capture-manual-shots.sh                    # every picture
#   ./scripts/capture-manual-shots.sh pareto-chart arima # only these
#
# Needs a GPU driver or a software one (on Linux: apt-get install
# mesa-vulkan-drivers). The pictures are listed in
# crates/app-gui/src/app/gui_tests/manual.rs.
set -euo pipefail
cd "$(dirname "$0")/.."
only=$(IFS=,; echo "$*")
ANOVACRAFT_GUI_SHOTS="${ANOVACRAFT_GUI_SHOTS:-$PWD/docs/manual/src/images}" \
MANUAL_SHOTS="$only" \
  cargo test -p app-gui manual_screenshots -- --ignored --nocapture

# Pictures taken partway through ordinary GUI tests (manual.rs TEST_SHOTS).
tests=$(MANUAL_SHOTS="$only" cargo test -p app-gui print_test_shots -- --ignored --nocapture 2>/dev/null \
  | sed -n 's/^TEST_SHOTS //p')
if [[ -n "$tests" ]]; then
  # shellcheck disable=SC2086
  ANOVACRAFT_GUI_SHOTS="${ANOVACRAFT_GUI_SHOTS:-$PWD/docs/manual/src/images}" MANUAL_TEST_SHOTS=1 \
    cargo test -p app-gui -- --test-threads=2 $tests ||
    echo "A test failed after its picture was taken (the window was resized for it); the pictures are written"
fi

# The Quick Guide's pictures (guide-*) live in its own images folder.
dir="${ANOVACRAFT_GUI_SHOTS:-$PWD/docs/manual/src/images}"
if [[ -z "${ANOVACRAFT_GUI_SHOTS:-}" ]]; then
  for png in "$dir"/guide-*.png; do
    [[ -e "$png" ]] && mv "$png" docs/guide/src/images/
  done
fi

# Shrink the pictures (a 256-colour palette suits screenshots and keeps the
# repository small), when Python's imaging library is installed.
python3 - "$dir" docs/guide/src/images "$@" <<'PY' || echo "Pillow is not installed (pip install pillow); pictures left uncompressed"
import pathlib, sys
from PIL import Image
folders, only = [pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])], set(sys.argv[3:])
for png in sorted(p for folder in folders for p in folder.glob("*.png")):
    if only and png.stem not in only:
        continue
    with Image.open(png) as im:
        if im.mode == "P":
            continue
        small = im.convert("RGB").quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    small.save(png, optimize=True)
PY
