#!/bin/bash
# Baut die Release-Dateien nach dist/release.
# Aufruf: scripts/release.sh <ffmpeg für macOS arm64> <ffmpeg.exe für Windows x64>
set -euo pipefail
cd "$(dirname "$0")/.."
MAC_FFMPEG=${1:?Pfad zu ffmpeg für macOS fehlt}
WIN_FFMPEG=${2:?Pfad zu ffmpeg.exe fehlt}
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
OUT=dist/release
rm -rf "$OUT"
mkdir -p "$OUT"

SPURLOS_EMBED_FFMPEG="$WIN_FFMPEG" cargo build --release --target x86_64-pc-windows-gnu
cp target/x86_64-pc-windows-gnu/release/spurlos.exe "$OUT/Spurlos-$VERSION-windows-x64.exe"

FFMPEG="$MAC_FFMPEG" ./scripts/bundle-macos.sh
STAGE=$(mktemp -d)
cp -R dist/Spurlos.app "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "Spurlos $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$OUT/Spurlos-$VERSION-macos-arm64.dmg"
rm -rf "$STAGE"

(cd "$OUT" && shasum -a 256 Spurlos-* > SHA256SUMS.txt)
ls -la "$OUT"
