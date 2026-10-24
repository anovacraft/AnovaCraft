#!/usr/bin/env bash
# Download NIST's AtmWtAg one-way ANOVA dataset into
# crates/stat-strd/data/anova/, where `cargo test -p stat-strd --test
# anova_certified` checks the one-way ANOVA against its certified values.
# (The other ten NIST ANOVA problems are bundled: SiRstv is transcribed and
# SmLs01–SmLs09 are generated from their published construction.)
#
# The file is a work of the U.S. Government (public domain):
# https://www.itl.nist.gov/div898/strd/anova/anova.html
set -euo pipefail
cd "$(dirname "$0")/.."
dest=crates/stat-strd/data/anova
mkdir -p "$dest"
out="$dest/AtmWtAg.dat"
curl -fsSL https://www.itl.nist.gov/div898/strd/anova/AtmWtAg.dat -o "$out.tmp"
# NIST serves the files with DOS line endings; keep them as text with LF.
tr -d '\r' < "$out.tmp" > "$out"
rm -f "$out.tmp"
grep -q "Between" "$out" || { echo "unexpected content in $out" >&2; exit 1; }
echo "Fetched $out."
echo "Check it with: cargo test -p stat-strd --test anova_certified -- --nocapture"
