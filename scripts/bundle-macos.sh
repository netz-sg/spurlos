#!/bin/bash
# Baut Spurlos.app nach dist/. Liegt unter FFMPEG ein statisches ffmpeg,
# wird es in die App gepackt. Sonst nutzt die App ffmpeg aus Homebrew.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cargo build --release
APP=dist/Spurlos.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/spurlos "$APP/Contents/MacOS/spurlos"
cp assets/Spurlos.icns "$APP/Contents/Resources/Spurlos.icns"
if [ -n "${FFMPEG:-}" ]; then cp "$FFMPEG" "$APP/Contents/Resources/ffmpeg"; fi
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Spurlos</string>
  <key>CFBundleDisplayName</key><string>Spurlos</string>
  <key>CFBundleIdentifier</key><string>org.sgn-tech.spurlos</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>spurlos</string>
  <key>CFBundleIconFile</key><string>Spurlos</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDocumentTypes</key>
  <array><dict>
    <key>CFBundleTypeRole</key><string>Editor</string>
    <key>LSItemContentTypes</key><array><string>public.image</string><string>public.movie</string></array>
  </dict></array>
</dict>
</plist>
PLIST
codesign --force --deep -s - "$APP"
echo "Fertig $APP"
