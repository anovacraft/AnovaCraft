#!/usr/bin/env bash
# Build AnovaCraft for 64-bit Windows from macOS (or Linux) by cross-compiling
# with the MinGW-w64 toolchain. The result is dist/AnovaCraft-windows-x64/
# (AnovaCraft.exe plus any runtime DLLs it needs) and a .zip of that folder
# (AnovaCraft-<version>-windows-x64.zip),
# ready to copy to a Windows PC and run — no installer needed.
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=x86_64-pc-windows-gnu
APP_NAME=AnovaCraft
OUT=dist/$APP_NAME-windows-x64
MINGW=x86_64-w64-mingw32
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
ZIP="$APP_NAME-${VERSION:-0.1.0}-windows-x64.zip"

if ! command -v "$MINGW-gcc" >/dev/null 2>&1; then
  echo "The MinGW-w64 cross compiler ($MINGW-gcc) is needed to build for Windows."
  if command -v brew >/dev/null 2>&1; then
    echo "Install it with:  brew install mingw-w64"
  else
    echo "Install the mingw-w64 package (macOS: https://brew.sh then 'brew install mingw-w64')."
  fi
  exit 1
fi
if command -v rustup >/dev/null 2>&1; then
  rustup target add "$TARGET" >/dev/null
else
  echo "rustup is needed to add the Windows target ($TARGET); see https://rustup.rs" >&2
  exit 1
fi

export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$MINGW-gcc"
export WINDRES="$MINGW-windres"
cargo build --release -p app-gui --target "$TARGET"

rm -rf "$OUT"
mkdir -p "$OUT"
cp "target/$TARGET/release/app-gui.exe" "$OUT/$APP_NAME.exe"
cp COPYRIGHT LICENSE-APACHE LICENSE-MIT THIRD-PARTY-NOTICES.txt "$OUT/"

# Ship any MinGW runtime libraries the executable links to (usually none).
if command -v "$MINGW-objdump" >/dev/null 2>&1; then
  for dll in $("$MINGW-objdump" -p "$OUT/$APP_NAME.exe" | awk '/DLL Name:/ {print $3}'); do
    case "$dll" in
      libgcc_s_seh-1.dll | libwinpthread-1.dll | libstdc++-6.dll)
        src="$("$MINGW-gcc" -print-file-name="$dll")"
        if [[ -f "$src" ]]; then cp "$src" "$OUT/"; else echo "warning: $dll is needed but was not found" >&2; fi
        ;;
    esac
  done
fi

(cd dist && rm -f "$ZIP" && zip -qr "$ZIP" "$APP_NAME-windows-x64")
echo "Built: $OUT/$APP_NAME.exe"
echo "Zipped: dist/$ZIP (copy to a Windows PC, unzip, and run $APP_NAME.exe)"
