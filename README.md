# Spurlos

Spurlos entfernt alle Metadaten aus Bildern und Videos. Das Programm läuft auf macOS und Windows und hat eine eigene Oberfläche mit Drag and Drop.

![Icon](assets/icon-256.png)

## Was entfernt wird

- EXIF mit Kamera, Objektiv, Datum und GPS-Position
- XMP, IPTC, Kommentare und Textfelder
- KI-Kennzeichnungen wie C2PA-Herkunftsnachweise (Content Credentials), der IPTC-Quelltyp `trainedAlgorithmicMedia`, Prompts und Workflows aus Stable Diffusion, ComfyUI oder Midjourney
- Farbprofile, Zeitstempel und DPI-Angaben
- angehängte Daten hinter dem Bildende, etwa Bewegtbild-Fotos oder Zusatzbilder
- bei Videos Titel, Ort, Aufnahmedatum, Gerät, Kapitel, Coverbilder, GPS- und Sensorspuren sowie Encoder-Signaturen im Videostrom

Erkennt Spurlos Hinweise auf KI-Erzeugung, steht in der Liste zusätzlich „KI-Kennzeichnung“ bei den entfernten Daten.

## Formate

Bilder sind JPEG, PNG, WebP, GIF, TIFF und BMP. JPEG, PNG, WebP und GIF werden verlustfrei bereinigt, die Bilddaten bleiben Byte für Byte gleich. Fotos mit EXIF-Drehung werden vorher richtig gedreht und neu kodiert, damit sie nach dem Entfernen nicht auf der Seite liegen.

Videos sind MP4, MOV, M4V, MKV, WebM, AVI und 3GP. Die Spuren werden ohne Qualitätsverlust umverpackt. Wahlweise kodiert Spurlos neu nach H.264 und AAC als MP4.

## Grenzen

Unsichtbare Wasserzeichen in den Pixeln selbst, etwa Googles SynthID, sind keine Metadaten. Die kann Spurlos nicht entfernen.

## Speichern

- neben dem Original als `name_clean.ext`
- Original ersetzen
- in einen frei gewählten Ordner

Geschrieben wird immer erst in eine Zwischendatei. Schlägt etwas fehl, bleibt das Original unangetastet.

## Bauen

Voraussetzung ist Rust. Für Videos braucht Spurlos ffmpeg. Gesucht wird neben der Programmdatei, in `Spurlos.app/Contents/Resources`, im `PATH` und in Homebrew.

```sh
cargo run --release

# macOS App nach dist/Spurlos.app, optional mit eingepacktem statischem ffmpeg
FFMPEG=/pfad/zu/ffmpeg ./scripts/bundle-macos.sh

# Windows von macOS aus, braucht mingw-w64 und das Rust-Ziel x86_64-pc-windows-gnu
./scripts/package-windows.sh /pfad/zu/ffmpeg.exe
```

## Kommandozeile

```sh
spurlos --cli datei.jpg ordner/ [--reencode]
```

Legt die bereinigten Dateien neben den Originalen ab und gibt aus, was entfernt wurde.
