#!/usr/bin/env bash
# Build RustTube.app (with libmpv and its libraries bundled) and a .dmg installer.
# Requires: Rust, `brew install mpv dylibbundler`.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
ARCH=$(uname -m)
OUT=dist
APP="$OUT/RustTube.app"
BREW_PREFIX=$(brew --prefix)

export LIBRARY_PATH="$BREW_PREFIX/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
cargo build --release

rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
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
  <key>LSApplicationCategoryType</key><string>public.app-category.video</string>
</dict></plist>
PLIST

# Copy libmpv and everything it links against into the app and fix up paths.
dylibbundler -od -b -cd \
  -x "$APP/Contents/MacOS/RustTube" \
  -d "$APP/Contents/Frameworks" \
  -p "@executable_path/../Frameworks/" >/dev/null

# dylibbundler can leave the same rpath twice in a file, which dyld rejects; keep one.
RP="@executable_path/../Frameworks/"
for f in "$APP/Contents/MacOS/RustTube" "$APP"/Contents/Frameworks/*.dylib; do
  while [ "$(otool -l "$f" | grep -c "path $RP ")" -gt 1 ]; do
    install_name_tool -delete_rpath "$RP" "$f" 2>/dev/null
  done
done

# Record the oldest macOS the bundled libraries support, so older systems get a
# clear "requires macOS X" message instead of a crash at launch.
MINOS=$(for f in "$APP/Contents/MacOS/RustTube" "$APP"/Contents/Frameworks/*.dylib; do
  otool -l "$f" | awk '/LC_BUILD_VERSION/{b=1} b&&/minos/{print $2; b=0}'
done | sort -t. -k1,1n -k2,2n | tail -1)
echo "Minimum macOS: $MINOS"
/usr/libexec/PlistBuddy -c "Set :LSMinimumSystemVersion $MINOS" "$APP/Contents/Info.plist"

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
