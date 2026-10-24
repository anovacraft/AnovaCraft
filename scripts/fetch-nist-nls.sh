#!/usr/bin/env bash
# Download the 27 NIST StRD nonlinear regression datasets into
# crates/stat-strd/data/nls/, where `cargo test -p stat-strd --test
# nls_certified` checks the nonlinear regression fits against their
# certified values (from both certified starting points).
#
# The files are a work of the U.S. Government (public domain):
# https://www.itl.nist.gov/div898/strd/nls/nls_main.shtml
set -euo pipefail
cd "$(dirname "$0")/.."
dest=crates/stat-strd/data/nls
mkdir -p "$dest"
base=https://www.itl.nist.gov/div898/strd/nls/data/LINKS/DATA
names=(Misra1a Chwirut2 Chwirut1 Lanczos3 Gauss1 Gauss2 DanWood Misra1b Kirby2 Hahn1
       Nelson MGH17 Lanczos1 Lanczos2 Gauss3 Misra1c Misra1d Roszman1 ENSO MGH09 Thurber
       BoxBOD Rat42 MGH10 Eckerle4 Rat43 Bennett5)
for name in "${names[@]}"; do
  out="$dest/$name.dat"
  echo "fetching $name"
  curl -fsSL "$base/$name.dat" -o "$out.tmp"
  # NIST serves the files with DOS line endings; keep them as text with LF.
  tr -d '\r' < "$out.tmp" > "$out"
  rm -f "$out.tmp"
  grep -q "Residual Sum of Squares" "$out" || { echo "unexpected content in $out" >&2; exit 1; }
done
echo "Fetched ${#names[@]} files into $dest."
echo "Check them with: cargo test -p stat-strd --test nls_certified -- --nocapture"
