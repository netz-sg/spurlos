//! Entfernt Metadaten aus Videos mit ffmpeg. Standardmäßig werden die Spuren
//! nur umverpackt, also ohne Qualitätsverlust. Datenspuren wie GPS-Tracks,
//! Kapitel, Coverbilder und Encoder-Signaturen im Videostrom fallen weg.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const EXTENSIONS: &[&str] = &["mp4", "m4v", "mov", "mkv", "webm", "avi", "3gp"];
const MP4_FAMILY: &[&str] = &["mp4", "m4v", "mov", "3gp"];

pub fn find_ffmpeg() -> Option<PathBuf> {
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        candidates.push(dir.join(name));
        candidates.push(dir.join("../Resources").join(name));
    }
    candidates.push(PathBuf::from(name));
    candidates.push("/opt/homebrew/bin/ffmpeg".into());
    candidates.push("/usr/local/bin/ffmpeg".into());
    candidates
        .into_iter()
        .find(|p| command(p).arg("-version").output().is_ok_and(|o| o.status.success()))
}

fn command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

use crate::report::Report;

/// Liest Tags, Spuren und Kennungen ohne die Datei zu verändern.
pub fn analyze(ffmpeg: &Path, input: &Path) -> Result<Report, String> {
    Ok(probe(ffmpeg, input)?.1)
}

fn probe(ffmpeg: &Path, input: &Path) -> Result<(String, Report), String> {
    let out = command(ffmpeg).arg("-hide_banner").arg("-i").arg(input).output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stderr);
    let mut codec = String::new();
    let mut report = Report::default();
    let mut camera = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.split_once("Video: ").map(|(_, r)| r) {
            if codec.is_empty() {
                codec = rest.split([' ', ',']).next().unwrap_or_default().to_string();
            }
            if line.contains("attached pic") {
                report.items.insert("Coverbild".into());
            }
        } else if line.contains("Data: ") {
            report.items.insert("Datenspur (z.B. GPS oder Sensoren)".into());
        } else if line.contains("Attachment: ") {
            report.items.insert("Anhänge".into());
        } else if line.trim_start().starts_with("Chapter #") {
            report.items.insert("Kapitel".into());
        } else if let Some((key, value)) = line.split_once(" : ") {
            let key = key.trim();
            let technical = key.eq_ignore_ascii_case("duration") || (key == "encoder" && value.trim() == "Lavf");
            if key.is_empty() || key == "language" || technical {
                continue;
            }
            report.note(key, value.as_bytes());
            let value = value.split(';').next().unwrap_or_default();
            let short = key.rsplit('.').next().unwrap_or(key).to_lowercase();
            match short.as_str() {
                "location" | "location-eng" | "iso6709" => report.detail("GPS-Position", value),
                "make" | "model" => camera.push(value.trim().to_string()),
                "creation_time" | "creationdate" | "date" => report.detail("Aufnahme", value),
                "software" => report.detail("Software", value),
                "artist" | "author" => report.detail("Urheber", value),
                "title" => report.detail("Titel", value),
                "comment" | "description" => report.detail("Kommentar", value),
                _ => {}
            }
        }
    }
    report.detail("Kamera", &camera.join(" "));
    if codec.is_empty() && !text.contains("Audio: ") {
        return Err("Keine Video- oder Tonspur gefunden".into());
    }
    if let Some(manifest) = c2pa(input) {
        report.note("C2PA-Herkunftsnachweis", &manifest);
    }
    if let Some(sig) = encoder_signature(input) {
        report.items.insert("Encoder-Signatur im Videostrom".into());
        report.detail("Encoder", &sig);
    }
    Ok((codec, report))
}

/// Sucht am Anfang und Ende der Datei nach Encoder-Kennungen von x264, x265
/// und ffmpeg. Dort liegen Kopfdaten und der erste Frame.
fn encoder_signature(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut data = Vec::new();
    (&mut f).take(32 << 20).read_to_end(&mut data).ok()?;
    if len > 32 << 20 {
        f.seek(SeekFrom::Start(len.saturating_sub(8 << 20).max(32 << 20))).ok()?;
        f.read_to_end(&mut data).ok()?;
    }
    for needle in [&b"x264 - core"[..], b"x265 (build", b"Lavc"] {
        let mut from = 0;
        while let Some(pos) = data[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from) {
            from = pos + 1;
            if needle == b"Lavc" && !data.get(pos + 4).is_some_and(u8::is_ascii_digit) {
                continue;
            }
            let tail = &data[pos..data.len().min(pos + 24)];
            let stop = |b: &u8| !(b.is_ascii_graphic() || *b == b' ' && needle != b"Lavc");
            let end = tail.iter().position(stop).unwrap_or(tail.len());
            return Some(String::from_utf8_lossy(&tail[..end]).to_string());
        }
    }
    None
}

pub fn strip(ffmpeg: &Path, input: &Path, output: &Path, reencode: bool) -> Result<Report, String> {
    let (codec, report) = probe(ffmpeg, input)?;
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
    let mut cmd = command(ffmpeg);
    cmd.args(["-hide_banner", "-v", "error", "-y", "-i"]).arg(input);
    cmd.args(["-map", "0:V?", "-map", "0:a?"]);
    if reencode {
        cmd.args(["-sn", "-c:v", "libx264", "-crf", "18", "-preset", "medium", "-pix_fmt", "yuv420p"]);
        cmd.args(["-c:a", "aac", "-b:a", "192k"]);
    } else {
        if ext != "avi" {
            cmd.args(["-map", "0:s?"]);
        }
        cmd.args(["-c", "copy"]);
    }
    cmd.args(["-dn", "-map_metadata", "-1", "-map_chapters", "-1"]);
    cmd.args(["-fflags", "+bitexact", "-flags:v", "+bitexact", "-flags:a", "+bitexact"]);
    cmd.args(["-metadata:s:v", "encoder=", "-metadata:s:a", "encoder="]);
    match if reencode { "h264" } else { codec.as_str() } {
        "h264" => { cmd.args(["-bsf:v", "filter_units=remove_types=6"]); }
        "hevc" => { cmd.args(["-bsf:v", "filter_units=remove_types=39|40"]); }
        _ => {}
    }
    if MP4_FAMILY.contains(&ext.as_str()) {
        cmd.args(["-empty_hdlr_name", "1", "-movflags", "+faststart"]);
    }
    let out = cmd.arg(output).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().last().unwrap_or("ffmpeg ist fehlgeschlagen").to_string());
    }
    if MP4_FAMILY.contains(&ext.as_str()) {
        scrub_mp4(output)?;
    }
    scrub_lavc(output)?;
    Ok(report)
}

/// Bereinigt den moov-Kopf direkt in der Datei. Die Herstellerkennung FFMP
/// wird genullt. Der Filter oben erreicht außerdem die SEI-Blöcke im hvcC-Kopf
/// nicht, dort steht bei x265 und vielen Kameras der Encoder. Der Block wird ohne
/// SEI neu geschrieben und die frei gewordenen Bytes mit einem free-Block
/// aufgefüllt, so bleibt die Größe gleich und alle Offsets bleiben gültig.
fn scrub_mp4(path: &Path) -> Result<(), String> {
    let e = |e: std::io::Error| e.to_string();
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(e)?;
    let len = f.metadata().map_err(e)?.len();
    let mut pos = 0;
    while pos + 8 <= len {
        let mut head = [0u8; 16];
        f.seek(SeekFrom::Start(pos)).map_err(e)?;
        f.read_exact(&mut head[..8]).map_err(e)?;
        let size = match u32::from_be_bytes(head[..4].try_into().unwrap()) as u64 {
            0 => len - pos,
            1 => {
                f.read_exact(&mut head[8..]).map_err(e)?;
                u64::from_be_bytes(head[8..].try_into().unwrap())
            }
            n => n,
        };
        if size < 8 || pos + size > len {
            return Ok(());
        }
        if &head[4..8] == b"moov" {
            let mut moov = vec![0; size as usize];
            f.seek(SeekFrom::Start(pos)).map_err(e)?;
            f.read_exact(&mut moov).map_err(e)?;
            if patch_moov(&mut moov) {
                f.seek(SeekFrom::Start(pos)).map_err(e)?;
                f.write_all(&moov).map_err(e)?;
            }
            return Ok(());
        }
        pos += size;
    }
    Ok(())
}

/// Ältere ffmpeg-Encoder schreiben ihre Version als Lavc-Text in den ersten
/// Audio-Frame. Der Block ist frei belegbar, das Nullen ändert am Ton nichts.
fn scrub_lavc(path: &Path) -> Result<(), String> {
    let e = |e: std::io::Error| e.to_string();
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(e)?;
    let mut head = Vec::new();
    (&mut f).take(32 << 20).read_to_end(&mut head).map_err(e)?;
    let mut from = 0;
    while let Some(pos) = head[from..].windows(4).position(|w| w == b"Lavc").map(|p| p + from) {
        from = pos + 4;
        let tail = &head[from..head.len().min(from + 16)];
        let Some(len) = tail.iter().position(|&b| b == 0) else { continue };
        if len == 0 || !tail[..len].iter().all(|b| b.is_ascii_digit() || *b == b'.') {
            continue;
        }
        f.seek(SeekFrom::Start(pos as u64)).map_err(e)?;
        f.write_all(&vec![0; 4 + len]).map_err(e)?;
    }
    Ok(())
}

/// Sucht auf oberster Ebene nach C2PA-Boxen und liefert den Anfang des
/// Manifests. ffmpeg übernimmt diese Boxen beim Umverpacken nicht.
fn c2pa(path: &Path) -> Option<Vec<u8>> {
    const C2PA_UUID: [u8; 16] = [0xD8, 0xFE, 0xC3, 0xD6, 0x1B, 0x0E, 0x48, 0x3C, 0x92, 0x97, 0x58, 0x28, 0x87, 0x7E, 0xC4, 0x81];
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut pos = 0;
    while pos + 8 <= len {
        let mut head = [0u8; 16];
        f.seek(SeekFrom::Start(pos)).ok()?;
        f.read_exact(&mut head[..8]).ok()?;
        let mut size = u32::from_be_bytes(head[..4].try_into().unwrap()) as u64;
        let mut body = 8;
        if size == 1 {
            f.read_exact(&mut head[8..]).ok()?;
            size = u64::from_be_bytes(head[8..].try_into().unwrap());
            body = 16;
        } else if size == 0 {
            size = len - pos;
        }
        if &head[4..8] == b"c2pa" || &head[4..8] == b"uuid" {
            let mut data = Vec::new();
            f.seek(SeekFrom::Start(pos + body)).ok()?;
            (&mut f).take((size - body).min(1 << 20)).read_to_end(&mut data).ok()?;
            if &head[4..8] == b"c2pa" || data.starts_with(&C2PA_UUID) {
                return Some(data);
            }
        }
        if size < 8 {
            return None;
        }
        pos += size;
    }
    None
}

fn patch_moov(d: &mut [u8]) -> bool {
    let mut changed = false;
    for i in 0..d.len().saturating_sub(4) {
        if &d[i..i + 4] == b"FFMP" {
            d[i..i + 4].fill(0);
            changed = true;
        }
    }
    let mut from = 0;
    while let Some(pos) = d[from..].windows(4).position(|w| w == b"hvcC").map(|p| p + from) {
        from = pos + 4;
        let Some(start) = pos.checked_sub(4) else { continue };
        let size = u32::from_be_bytes(d[start..pos].try_into().unwrap()) as usize;
        let Some(clean) = d.get(start + 8..start + size).and_then(hvcc_without_sei) else { continue };
        let gap = size - 8 - clean.len();
        if gap < 8 {
            continue;
        }
        let mut out = ((clean.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(b"hvcC");
        out.extend_from_slice(&clean);
        out.extend_from_slice(&(gap as u32).to_be_bytes());
        out.extend_from_slice(b"free");
        out.resize(size, 0);
        d[start..start + size].copy_from_slice(&out);
        changed = true;
    }
    changed
}

fn hvcc_without_sei(p: &[u8]) -> Option<Vec<u8>> {
    let mut out = p.get(..23)?.to_vec();
    let (mut i, mut kept) = (23, 0u8);
    for _ in 0..p[22] {
        let kind = p.get(i)? & 0x3F;
        let count = u16::from_be_bytes(p.get(i + 1..i + 3)?.try_into().ok()?) as usize;
        let mut end = i + 3;
        for _ in 0..count {
            end += 2 + u16::from_be_bytes(p.get(end..end + 2)?.try_into().ok()?) as usize;
        }
        if !matches!(kind, 39 | 40) {
            out.extend_from_slice(p.get(i..end)?);
            kept += 1;
        }
        i = end;
    }
    out[22] = kept;
    (out.len() < p.len()).then_some(out)
}
