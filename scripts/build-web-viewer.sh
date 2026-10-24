#!/usr/bin/env bash
# Build the web dashboard viewer: app-gui for WebAssembly, run through
# wasm-bindgen, into target/web-viewer (anovacraft_viewer.js and
# anovacraft_viewer_bg.wasm). "Publish dashboard as web app…" copies these
# two files into the folder it writes; it finds target/web-viewer beside a
# Cargo build of the app, or the folder named by ANOVACRAFT_WEB_VIEWER.
#
#   ./scripts/build-web-viewer.sh            # build target/web-viewer
#   ./scripts/build-web-viewer.sh --serve    # also publish the Greenhouse
#                                            # sample there and serve it on
#                                            # http://localhost:8000
#
# Needs the wasm32-unknown-unknown target (added with rustup if missing) and
# wasm-bindgen-cli at the same version as the wasm-bindgen crate in
# Cargo.lock; it installs both the first time if they are missing.
# wasm-opt (from binaryen), when installed, shrinks the module further.
# See docs/manual/src/maintaining/web-build.md. Not run by CI.
set -euo pipefail
cd "$(dirname "$0")/.."

target=wasm32-unknown-unknown
out=target/web-viewer
if command -v rustup >/dev/null 2>&1 && ! rustup target list --installed | grep -qx "$target"; then
  echo "Installing the $target target"
  rustup target add "$target"
fi

# wasm-bindgen-cli must match the wasm-bindgen crate exactly.
want=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' Cargo.lock)
have=$(wasm-bindgen --version 2>/dev/null | awk '{ print $2 }' || true)
if [[ "$have" != "$want" ]]; then
  echo "Installing wasm-bindgen $want (found: ${have:-none})"
  cargo install wasm-bindgen-cli --version "$want" --locked
fi

echo "== Building app-gui for $target (wasm-release profile)"
cargo build --profile wasm-release --target "$target" -p app-gui

echo "== wasm-bindgen"
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" --out-name anovacraft_viewer \
  "target/$target/wasm-release/app-gui.wasm"

if command -v wasm-opt >/dev/null 2>&1; then
  echo "== wasm-opt"
  wasm-opt -O2 --enable-bulk-memory --enable-nontrapping-float-to-int \
    "$out/anovacraft_viewer_bg.wasm" -o "$out/anovacraft_viewer_bg.wasm"
fi
ls -l "$out"

if [[ "${1:-}" == "--serve" ]]; then
  site=target/web-viewer-sample
  echo "== Publishing the Greenhouse sample to $site"
  cargo run --release -p app-gui -- --publish-sample "$site"
  echo "Serving $site on http://localhost:8000 (Ctrl-C stops it)"
  cd "$site" && python3 -m http.server 8000
fi
