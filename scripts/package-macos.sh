#!/usr/bin/env bash
# Build RustTube.app and a .dmg installer. Requires only Rust (uses the system WebKit).
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
ARCH=$(uname -m)
OUT=dist
APP="$OUT/RustTube.app"

cargo build --release

rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/rusttube "$APP/Contents/MacOS/RustTube"

# App icon (.icns) from icons/icon.png
ICONSET="$OUT/icon.iconset"
mkdir -p "$ICONSET"
for s in 16 32 64 128 256 512; do
  sips -z $s $s icons/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  [ $d -le 1024 ] && sips -z $d $d icons/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null || true
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/RustTube.icns"
rm -rf "$ICONSET"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>RustTube</string>
  <key>CFBundleDisplayName</key><string>RustTube</string>
  <key>CFBundleIdentifier</key><string>org.rusttube.app</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>RustTube</string>
  <key>CFBundleIconFile</key><string>RustTube</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAppTransportSecurity</key><dict><key>NSAllowsLocalNetworking</key><true/></dict>
  <key>LSApplicationCategoryType</key><string>public.app-category.video</string>
</dict></plist>
PLIST

# Ad-hoc sign (required to run on Apple Silicon). Replace "-" with a Developer ID
# identity to distribute without Gatekeeper warnings.
codesign --force --deep --sign - "$APP"

# DMG with an Applications shortcut.
STAGE="$OUT/dmg"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
DMG="$OUT/RustTube-$VERSION-macos-$ARCH.dmg"
hdiutil create -volname "RustTube" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"
echo "Built $DMG"
