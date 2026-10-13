//! Entfernt Metadaten aus Bildern. JPEG, PNG, WebP und GIF werden verlustfrei
//! bereinigt, indem nur die Bildblöcke übernommen werden. Andere Formate und
//! gedrehte Fotos werden neu kodiert.

use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};

pub const EXTENSIONS: &[&str] = &["jpg", "jpeg", "jfif", "png", "apng", "webp", "gif", "tif", "tiff", "bmp"];

pub type Removed = BTreeSet<String>;
type Res<T> = Result<T, String>;

pub fn strip(input: &Path, output: &Path) -> Res<Removed> {
    let data = std::fs::read(input).map_err(|e| e.to_string())?;
    let format = image::guess_format(&data).map_err(|_| "Unbekanntes Bildformat".to_string())?;
    let mut removed = Removed::new();
    let clean = if orientation(&data)? != Orientation::NoTransforms {
        removed.insert("Ausrichtung (Bild wurde gedreht)".into());
        reencode(&data, format, &mut removed)?
    } else {
        match format {
            ImageFormat::Jpeg => jpeg(&data, &mut removed)?,
            ImageFormat::Png => png(&data, &mut removed)?,
            ImageFormat::WebP => webp(&data, &mut removed)?,
            ImageFormat::Gif => gif(&data, &mut removed)?,
            _ => reencode(&data, format, &mut removed)?,
        }
    };
    std::fs::write(output, clean).map_err(|e| e.to_string())?;
    Ok(removed)
}

fn decoder(data: &[u8]) -> Res<impl ImageDecoder + '_> {
    let reader = ImageReader::new(Cursor::new(data)).with_guessed_format().map_err(|e| e.to_string())?;
    reader.into_decoder().map_err(|e| format!("Bild nicht lesbar ({e})"))
}

fn orientation(data: &[u8]) -> Res<Orientation> {
    Ok(decoder(data)?.orientation().unwrap_or(Orientation::NoTransforms))
}

fn reencode(data: &[u8], format: ImageFormat, removed: &mut Removed) -> Res<Vec<u8>> {
    let mut dec = decoder(data)?;
    let turn = dec.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(dec).map_err(|e| e.to_string())?;
    img.apply_orientation(turn);
    let mut out = Cursor::new(Vec::new());
    let written = match format {
        ImageFormat::Jpeg => DynamicImage::ImageRgb8(img.to_rgb8())
            .write_with_encoder(JpegEncoder::new_with_quality(&mut out, 95)),
        _ => img.write_to(&mut out, format),
    };
    written.map_err(|e| e.to_string())?;
    note(removed, "Alle Metadaten (neu kodiert)", &data[..data.len().min(256 * 1024)]);
    Ok(out.into_inner())
}

const C2PA: &str = "C2PA-Herkunftsnachweis";

fn note(removed: &mut Removed, label: &str, data: &[u8]) {
    removed.insert(label.into());
    if label == C2PA || crate::ai::marker(data) {
        removed.insert(crate::ai::LABEL.into());
    }
}

fn broken() -> String {
    "Datei ist beschädigt oder unvollständig".into()
}

fn jpeg(d: &[u8], removed: &mut Removed) -> Res<Vec<u8>> {
    if !d.starts_with(&[0xFF, 0xD8]) {
        return Err(broken());
    }
    let mut out = vec![0xFF, 0xD8];
    let mut i = 2;
    loop {
        while d.get(i) == Some(&0xFF) && d.get(i + 1) == Some(&0xFF) {
            i += 1;
        }
        if i >= d.len() {
            out.extend_from_slice(&[0xFF, 0xD9]);
            return Ok(out);
        }
        if d[i] != 0xFF || i + 1 >= d.len() {
            return Err(broken());
        }
        let marker = d[i + 1];
        if marker == 0xD9 {
            out.extend_from_slice(&[0xFF, 0xD9]);
            if i + 2 < d.len() {
                note(removed, "Angehängte Daten", &d[i + 2..]);
            }
            return Ok(out);
        }
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            out.extend_from_slice(&d[i..i + 2]);
            i += 2;
            continue;
        }
        let len = d.get(i + 2..i + 4).map(|b| u16::from_be_bytes([b[0], b[1]]) as usize).ok_or_else(broken)?;
        let end = (i + 2 + len).min(d.len());
        let body = &d[(i + 4).min(end)..end];
        match marker {
            0xEE if body.starts_with(b"Adobe") => out.extend_from_slice(&d[i..end]),
            0xE0..=0xEF | 0xFE => note(removed, jpeg_label(marker, body), body),
            _ => out.extend_from_slice(&d[i..end]),
        }
        i = end;
        if marker == 0xDA {
            let start = i;
            while i + 1 < d.len() && !(d[i] == 0xFF && d[i + 1] != 0 && !(0xD0..=0xD7).contains(&d[i + 1])) {
                i += 1;
            }
            if i + 1 >= d.len() {
                i = d.len();
            }
            out.extend_from_slice(&d[start..i]);
        }
    }
}

fn jpeg_label(marker: u8, body: &[u8]) -> &'static str {
    match marker {
        0xE1 if body.starts_with(b"Exif") => "EXIF (Kamera, Datum, GPS)",
        0xE1 if body.starts_with(b"http://ns.adobe.com/xap") => "XMP",
        0xE2 if body.starts_with(b"ICC_PROFILE") => "Farbprofil (ICC)",
        0xE2 if body.starts_with(b"MPF") => "Zusatzbilder (MPF)",
        0xED => "IPTC",
        0xEB => C2PA,
        0xE0 => "JFIF-Kopf",
        0xFE => "Kommentar",
        _ => "Herstellerdaten",
    }
}

const PNG_SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
const PNG_KEEP: &[&[u8]] = &[b"IHDR", b"PLTE", b"tRNS", b"IDAT", b"IEND", b"acTL", b"fcTL", b"fdAT"];

fn png(d: &[u8], removed: &mut Removed) -> Res<Vec<u8>> {
    if !d.starts_with(PNG_SIG) {
        return Err(broken());
    }
    let mut out = PNG_SIG.to_vec();
    let mut i = PNG_SIG.len();
    while i + 12 <= d.len() {
        let len = u32::from_be_bytes(d[i..i + 4].try_into().unwrap()) as usize;
        let kind = &d[i + 4..i + 8];
        let end = i + 12 + len;
        if end > d.len() {
            return Err(broken());
        }
        if PNG_KEEP.contains(&kind) {
            out.extend_from_slice(&d[i..end]);
        } else {
            note(removed, png_label(kind), &d[i + 8..end - 4]);
        }
        i = end;
        if kind == b"IEND" {
            break;
        }
    }
    if i < d.len() {
        note(removed, "Angehängte Daten", &d[i..]);
    }
    Ok(out)
}

fn png_label(kind: &[u8]) -> &'static str {
    match kind {
        b"tEXt" | b"zTXt" | b"iTXt" => "Textfelder",
        b"eXIf" => "EXIF (Kamera, Datum, GPS)",
        b"tIME" => "Zeitstempel",
        b"pHYs" => "Auflösung (DPI)",
        b"iCCP" | b"sRGB" | b"gAMA" | b"cHRM" | b"cICP" | b"mDCv" | b"cLLi" => "Farbinformationen",
        b"caBX" => C2PA,
        _ => "Sonstige Blöcke",
    }
}

const WEBP_KEEP: &[&[u8]] = &[b"VP8 ", b"VP8L", b"VP8X", b"ALPH", b"ANIM", b"ANMF"];

fn webp(d: &[u8], removed: &mut Removed) -> Res<Vec<u8>> {
    if d.len() < 12 || &d[0..4] != b"RIFF" || &d[8..12] != b"WEBP" {
        return Err(broken());
    }
    let mut body = Vec::new();
    let mut i = 12;
    while i + 8 <= d.len() {
        let id = &d[i..i + 4];
        let len = u32::from_le_bytes(d[i + 4..i + 8].try_into().unwrap()) as usize;
        let end = (i + 8 + len + (len & 1)).min(d.len());
        match id {
            b"EXIF" => note(removed, "EXIF (Kamera, Datum, GPS)", &d[i..end]),
            b"XMP " => note(removed, "XMP", &d[i..end]),
            b"ICCP" => note(removed, "Farbprofil (ICC)", &d[i..end]),
            b"C2PA" => note(removed, C2PA, &d[i..end]),
            _ if WEBP_KEEP.contains(&id) => body.extend_from_slice(&d[i..end]),
            _ => note(removed, "Sonstige Blöcke", &d[i..end]),
        }
        i = end;
    }
    if body.starts_with(b"VP8X") && body.len() > 8 {
        body[8] &= !(0x20 | 0x08 | 0x04);
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&body);
    Ok(out)
}

fn gif(d: &[u8], removed: &mut Removed) -> Res<Vec<u8>> {
    if d.len() < 13 || !(d.starts_with(b"GIF87a") || d.starts_with(b"GIF89a")) {
        return Err(broken());
    }
    let mut i = 13 + color_table(d[10]);
    let mut out = d.get(..i).ok_or_else(broken)?.to_vec();
    loop {
        match d.get(i) {
            Some(0x3B) => {
                out.push(0x3B);
                return Ok(out);
            }
            Some(0x21) => {
                let label = *d.get(i + 1).ok_or_else(broken)?;
                let end = skip_blocks(d, i + 2)?;
                let app = d.get(i + 3..i + 14).unwrap_or_default();
                match label {
                    0xF9 | 0x01 => out.extend_from_slice(&d[i..end]),
                    0xFF if app == b"NETSCAPE2.0" || app == b"ANIMEXTS1.0" => out.extend_from_slice(&d[i..end]),
                    0xFE => note(removed, "Kommentar", &d[i..end]),
                    0xFF if app.starts_with(b"XMP Data") => note(removed, "XMP", &d[i..end]),
                    0xFF if app.starts_with(b"ICCRGBG1") => note(removed, "Farbprofil (ICC)", &d[i..end]),
                    _ => note(removed, "Anwendungsdaten", &d[i..end]),
                }
                i = end;
            }
            Some(0x2C) => {
                let flags = *d.get(i + 9).ok_or_else(broken)?;
                let end = skip_blocks(d, i + 11 + color_table(flags))?;
                out.extend_from_slice(&d[i..end]);
                i = end;
            }
            None => {
                out.push(0x3B);
                return Ok(out);
            }
            _ => return Err(broken()),
        }
    }
}

fn color_table(flags: u8) -> usize {
    if flags & 0x80 == 0 { 0 } else { 3 << ((flags & 7) + 1) }
}

fn skip_blocks(d: &[u8], mut i: usize) -> Res<usize> {
    loop {
        let n = *d.get(i).ok_or_else(broken)? as usize;
        i += 1 + n;
        if n == 0 {
            return Ok(i);
        }
    }
}
