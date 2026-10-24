#!/usr/bin/env bash
# Check the two user manuals the way CI does: the Reference Manual
# (docs/manual) and the Quick Guide (docs/guide).
#
#   ./scripts/check-manual.sh              # build and check the book
#   ./scripts/check-manual.sh origin/main  # also warn if user-facing code
#                                          # changed since that ref without a
#                                          # manual change
#
# Fails on: a book that does not build, a link to a page that does not
# exist, a missing image, a picture whose proportions are not a normal
# window's (or whose size is forced in the page), or a screenshot marker not
# in the checklist.
set -euo pipefail
cd "$(dirname "$0")/.."

src=docs/manual/src
status=0

if command -v mdbook >/dev/null 2>&1; then
  mdbook build docs/manual
  mdbook build docs/guide
else
  echo "mdbook is not installed (cargo install mdbook); skipping the build"
fi

# Relative links between pages, and images, must point at real files.
# Pictures must have a normal window's proportions (wider than tall, between
# 5:4 and 2:1) and keep them: no width or height set in the page.
python3 - "$src" docs/guide/src <<'PY' || status=1
import pathlib, re, struct, sys
bad = []
pages = [p for root in sys.argv[1:] for p in pathlib.Path(root).rglob("*.md")]
for page in pages:
    raw = page.read_text(encoding="utf-8")
    for tag in re.findall(r"<img\b[^>]*>", raw):
        if re.search(r"\b(width|height)\s*=", tag) or re.search(r"style\s*=[^>]*(width|height)", tag):
            bad.append(f"{page}: picture with a forced size: {tag}")
    for target in re.findall(r"!\[[^\]]*\]\(([^)\s]+\.png)\)", raw):
        png = (page.parent / target).resolve()
        if png.exists():
            with open(png, "rb") as f:
                head = f.read(24)
            w, h = struct.unpack(">II", head[16:24])
            if not 1.25 <= w / h <= 2.0:
                bad.append(f"{page}: {target} is {w} × {h}; use a whole window or a crop between 5:4 and 2:1")
for page in pages:
    text = page.read_text(encoding="utf-8")
    # Skip fenced code blocks.
    text = re.sub(r"```.*?```", "", text, flags=re.S)
    text = re.sub(r"`[^`\n]*`", "", text)
    for target in re.findall(r"\]\(([^)\s]+)\)", text):
        if re.match(r"[a-z]+:", target) or target.startswith("#"):
            continue
        path = target.split("#", 1)[0]
        if not (page.parent / path).resolve().exists():
            bad.append(f"{page}: link to missing {target}")
for b in bad:
    print(f"::error::{b}")
sys.exit(1 if bad else 0)
PY

# Every screenshot marker must be in the checklist.
checklist=$src/maintaining/screenshots.md
pending=0
while IFS= read -r id; do
  pending=$((pending + 1))
  if ! grep -q "| \`$id\` |" "$checklist"; then
    echo "::error::screenshot marker '$id' is not listed in $checklist"
    status=1
  fi
done < <(grep -rhoE 'Screenshot to add\*\* \(`[a-z0-9-]+`\)' "$src" docs/guide/src --exclude=screenshots.md \
           | sed -E 's/.*`(.*)`.*/\1/')
echo "Screenshots still to capture: $pending"

# Warn when user-facing code changed but the manual did not.
if [[ $# -ge 1 ]]; then
  changed=$(git diff --name-only "$1"...HEAD)
  ui=$(grep -E '^(crates/(app-gui|app-model|stat-script)/src/|FEATURES\.md$)' <<<"$changed" || true)
  docs=$(grep -E '^(docs/manual/|docs/guide/|docs/formulas\.md$|docs/scripting\.md$)' <<<"$changed" || true)
  if [[ -n "$ui" && -z "$docs" ]]; then
    echo "::warning title=User manual not updated::This change touches user-facing code but not docs/manual. If anything a user can see changed (including features already considered done), update its manual page. Files: $(tr '\n' ' ' <<<"$ui")"
  fi
fi

exit $status
