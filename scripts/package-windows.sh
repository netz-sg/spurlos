#!/bin/bash
# Baut Spurlos für Windows und packt es mit ffmpeg.exe nach dist/Spurlos-Windows.zip.
# Aufruf mit dem Pfad zu einer statischen ffmpeg.exe als erstem Argument.
set -euo pipefail
cd "$(dirname "$0")/.."
FFMPEG_EXE=${1:?Pfad zu ffmpeg.exe fehlt}
cargo build --release --target x86_64-pc-windows-gnu
OUT=dist/Spurlos-Windows
rm -rf "$OUT" "$OUT.zip"
mkdir -p "$OUT"
cp target/x86_64-pc-windows-gnu/release/spurlos.exe "$OUT/Spurlos.exe"
cp "$FFMPEG_EXE" "$OUT/ffmpeg.exe"
LICENSE_FILE="$(dirname "$FFMPEG_EXE")/LICENSE"
[ -f "$LICENSE_FILE" ] && cp "$LICENSE_FILE" "$OUT/ffmpeg-LICENSE.txt"
printf 'Spurlos entfernt alle Metadaten aus Bildern und Videos.\r\n\r\nSpurlos.exe starten und Dateien ins Fenster ziehen.\r\nffmpeg.exe muss im selben Ordner bleiben, sonst lassen sich keine Videos bereinigen.\r\n' > "$OUT/LIESMICH.txt"
(cd dist && zip -qr Spurlos-Windows.zip Spurlos-Windows)
echo "Fertig $OUT.zip"
