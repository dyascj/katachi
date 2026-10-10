#!/usr/bin/env bash
# Build target/Katachi.app. Pass --install to copy it into /Applications.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cargo build --release ${TARGET:+--target "$TARGET"}
BIN="target/${TARGET:+$TARGET/}release/katachi"
APP=target/Katachi.app

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/katachi"
cp assets/icon/Katachi.icns "$APP/Contents/Resources/"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Katachi</string>
  <key>CFBundleDisplayName</key><string>Katachi</string>
  <key>CFBundleIdentifier</key><string>com.dyascj.katachi</string>
  <key>CFBundleExecutable</key><string>katachi</string>
  <key>CFBundleIconFile</key><string>Katachi</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>Katachi document</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSHandlerRank</key><string>Owner</string>
      <key>CFBundleTypeExtensions</key><array><string>katachi</string></array>
      <key>CFBundleTypeIconFile</key><string>Katachi</string>
    </dict>
    <dict>
      <key>CFBundleTypeName</key><string>Figma file</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Alternate</string>
      <key>CFBundleTypeExtensions</key><array><string>fig</string></array>
    </dict>
  </array>
</dict>
</plist>
PLIST
# Ad-hoc signature so Apple Silicon will launch a locally built app.
codesign --force --deep --sign - "$APP" 2>/dev/null
echo "Built $APP"

if [[ "${1:-}" == "--install" ]]; then
  rm -rf /Applications/Katachi.app
  cp -R "$APP" /Applications/
  # Let Finder know Katachi opens .katachi (and .fig) files.
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f /Applications/Katachi.app || true
  echo "Installed /Applications/Katachi.app"
fi
