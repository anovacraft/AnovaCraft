#!/usr/bin/env bash
# Check that the parts of AnovaCraft that can run in a web browser compile
# for WebAssembly (wasm32-unknown-unknown). See
# docs/manual/src/maintaining/web-build.md. app-gui is the web dashboard
# viewer there; scripts/build-web-viewer.sh builds it.
#
#   ./scripts/check-web-build.sh
#
# Not run by CI. Needs the target: rustup target add wasm32-unknown-unknown
set -euo pipefail
cd "$(dirname "$0")/.."

target=wasm32-unknown-unknown
if command -v rustup >/dev/null 2>&1 && ! rustup target list --installed | grep -qx "$target"; then
  echo "Installing the $target target"
  rustup target add "$target"
fi

engine=(
  stat-core stat-data stat-linear stat-strd stat-quality stat-discrete stat-doe
  stat-multivariate stat-predictive stat-reliability stat-inference stat-mixed
  stat-viz stat-timeseries
)
args=()
for p in "${engine[@]}"; do args+=(-p "$p"); done

echo "== Engine crates"
cargo check --target "$target" "${args[@]}"

# Database drivers and web access (sockets, native TLS, bundled SQLite) are
# app-model's default features; a browser build leaves them out.
echo "== app-model and stat-script (no database drivers or web access)"
cargo check --target "$target" -p app-model -p stat-script --no-default-features

# stat-viz's egui drawing compiles for the web too.
echo "== stat-viz with its egui feature"
cargo check --target "$target" -p stat-viz --features egui

# The application itself, which runs in a browser as the dashboard viewer.
echo "== app-gui (the web dashboard viewer)"
cargo check --target "$target" -p app-gui

echo "The web build check passed."
