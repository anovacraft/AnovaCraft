#!/usr/bin/env bash
# Build "AnovaCraft.app" for macOS from this workspace.
#
#   ./scripts/build-macos-app.sh            # app for this Mac's architecture
#   ./scripts/build-macos-app.sh universal  # one app for Apple silicon and Intel
#
# The app is built with the `dist` profile (thin LTO, one codegen unit:
# slower to build than --release, faster to run) and written to dist/. It is
# not code-signed: the first time, allow it under System Settings ▸ Privacy &
# Security ▸ Open Anyway (or right-click ▸ Open on macOS 14 and earlier). A zip
# of the app for downloads is written beside it.
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME="AnovaCraft"
BUNDLE_ID="org.anovacraft.app"
# The version lives in the workspace manifest ([workspace.package]).
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
VERSION="${VERSION:-0.1.0}"

ARCH_LABEL="$(uname -m)"
if [[ "${1:-}" == "universal" ]]; then
  ARCH_LABEL=universal
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --profile dist -p app-gui --target aarch64-apple-darwin
  cargo build --profile dist -p app-gui --target x86_64-apple-darwin
  mkdir -p target/universal
  lipo -create -output target/universal/app-gui \
    target/aarch64-apple-darwin/dist/app-gui target/x86_64-apple-darwin/dist/app-gui
  BINARY=target/universal/app-gui
else
  cargo build --profile dist -p app-gui
  BINARY=target/dist/app-gui
fi

APP="dist/$APP_NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/examples"
cp "$BINARY" "$APP/Contents/MacOS/$APP_NAME"
cp -R examples/. "$APP/Contents/Resources/examples/" 2>/dev/null || true
# Licences: AnovaCraft's own and the third-party notices (also built into the
# binary, under Help ▸ About AnovaCraft ▸ Open-source licences).
cp COPYRIGHT LICENSE-APACHE LICENSE-MIT THIRD-PARTY-NOTICES.txt "$APP/Contents/Resources/"

# App icon: an .icns built from the 1024-pixel artwork.
ICONSET="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$ICONSET"
SRC=crates/app-gui/assets/icon-1024.png
for size in 16 32 128 256 512; do
  sips -z $size $size "$SRC" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) "$SRC" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>$APP_NAME</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <!-- AnovaCraft projects (.acraft) belong to this app; data files can be opened with it. -->
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>AnovaCraft Project</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSHandlerRank</key><string>Owner</string>
      <key>CFBundleTypeIconFile</key><string>AppIcon</string>
      <key>LSItemContentTypes</key><array><string>org.anovacraft.project</string></array>
    </dict>
    <dict>
      <key>CFBundleTypeName</key><string>Data File</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Alternate</string>
      <key>LSItemContentTypes</key>
      <array>
        <string>public.comma-separated-values-text</string>
        <string>public.tab-separated-values-text</string>
        <string>public.json</string>
        <string>org.openxmlformats.spreadsheetml.sheet</string>
        <string>com.microsoft.excel.xls</string>
        <string>org.oasis-open.opendocument.spreadsheet</string>
      </array>
    </dict>
  </array>
  <key>UTExportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeIdentifier</key><string>org.anovacraft.project</string>
      <key>UTTypeDescription</key><string>AnovaCraft Project</string>
      <key>UTTypeIconFile</key><string>AppIcon</string>
      <key>UTTypeConformsTo</key><array><string>public.data</string><string>public.content</string></array>
      <key>UTTypeTagSpecification</key>
      <dict><key>public.filename-extension</key><array><string>acraft</string></array></dict>
    </dict>
  </array>
</dict>
</plist>
PLIST

# Ad-hoc signature so Apple silicon will launch it locally.
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true

# Tell Finder and the Dock about the (possibly changed) icon: macOS caches app
# icons by bundle and otherwise keeps showing a blank or outdated one.
touch "$APP"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
[[ -x "$LSREGISTER" ]] && "$LSREGISTER" -f "$APP" >/dev/null 2>&1 || true

# A zip for downloads (ditto keeps the bundle's signature and attributes).
ZIP="dist/$APP_NAME-$VERSION-macos-$ARCH_LABEL.zip"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"

echo "Built: $APP"
echo "Zipped: $ZIP"
echo "Install by dragging it to /Applications, or run: open \"$APP\""
