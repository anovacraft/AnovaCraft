#!/usr/bin/env bash
# Build the PDFs of the two manuals that the app carries (Help ▸ Quick guide
# and Help ▸ Reference manual) into crates/app-gui/assets/manuals/, with the
# docs stamp the app's tests compare against the Markdown. Run it in every
# change that touches docs/manual, docs/guide, docs/formulas.md or
# docs/scripting.md, and commit the three files it writes.
#
#   ./scripts/build-manual-pdfs.sh
#
# Needs mdBook 0.4.40 (`mdbook`, or MDBOOK=/path/to/mdbook) and Node with
# Playwright and its Chromium (npm install -g playwright; npx playwright
# install chromium). MANUAL_IMAGE_WIDTH and MANUAL_JPEG_QUALITY tune how the
# screenshots are compressed (defaults 1100 and 0.65).
set -euo pipefail
cd "$(dirname "$0")/.."

mdbook=${MDBOOK:-mdbook}
out=crates/app-gui/assets/manuals
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

"$mdbook" build docs/guide -d "$work/guide"
"$mdbook" build docs/manual -d "$work/manual"
node scripts/book-pdf.mjs "$work/guide" "$out/quick-guide.pdf" "AnovaCraft Quick Guide" docs/guide/src
node scripts/book-pdf.mjs "$work/manual" "$out/reference-manual.pdf" "AnovaCraft Reference Manual" docs/manual/src
node scripts/manual-stamp.mjs > "$out/docs-stamp.txt"
ls -lh "$out"
