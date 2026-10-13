//! Entfernt Metadaten aus Videos mit ffmpeg. Standardmäßig werden die Spuren
//! nur umverpackt, also ohne Qualitätsverlust. Datenspuren wie GPS-Tracks,
//! Kapitel, Coverbilder und Encoder-Signaturen im Videostrom fallen weg.

use std::collections::BTreeSet;
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

struct Probe {
    codec: String,
    found: BTreeSet<String>,
}

fn probe(ffmpeg: &Path, input: &Path) -> Result<Probe, String> {
    let out = command(ffmpeg).arg("-hide_banner").arg("-i").arg(input).output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stderr);
    let mut codec = String::new();
    let mut found = BTreeSet::new();
    for line in text.lines() {
        if let Some(rest) = line.split_once("Video: ").map(|(_, r)| r) {
            if codec.is_empty() {
                codec = rest.split([' ', ',']).next().unwrap_or_default().to_string();
            }
            if line.contains("attached pic") {
                found.insert("Coverbild".into());
            }
        } else if line.contains("Data: ") {
            found.insert("Datenspur (z.B. GPS oder Sensoren)".into());
        } else if line.contains("Attachment: ") {
            found.insert("Anhänge".into());
        } else if line.trim_start().starts_with("Chapter #") {
            found.insert("Kapitel".into());
        } else if let Some((key, value)) = line.split_once(" : ") {
            if crate::ai::marker(value.as_bytes()) {
                found.insert(crate::ai::LABEL.into());
            }
            let key = key.trim();
            if !key.is_empty() && key != "language" {
                found.insert(key.to_string());
            }
        }
    }
    if codec.is_empty() && !text.contains("Audio: ") {
        return Err("Keine Video- oder Tonspur gefunden".into());
    }
    Ok(Probe { codec, found })
}

pub fn strip(ffmpeg: &Path, input: &Path, output: &Path, reencode: bool) -> Result<BTreeSet<String>, String> {
    let Probe { codec, mut found } = probe(ffmpeg, input)?;
    if has_c2pa(input) {
        found.insert("C2PA-Herkunftsnachweis".into());
        found.insert(crate::ai::LABEL.into());
    }
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
    if matches!(codec.as_str(), "h264" | "hevc") {
        found.insert("Encoder-Signatur im Videostrom".into());
    }
    Ok(found)
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

/// Sucht auf oberster Ebene nach C2PA-Boxen. ffmpeg übernimmt diese beim
/// Umverpacken ohnehin nicht, die Suche dient nur der Anzeige.
fn has_c2pa(path: &Path) -> bool {
    const C2PA_UUID: [u8; 16] = [0xD8, 0xFE, 0xC3, 0xD6, 0x1B, 0x0E, 0x48, 0x3C, 0x92, 0x97, 0x58, 0x28, 0x87, 0x7E, 0xC4, 0x81];
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let len = f.metadata().map_or(0, |m| m.len());
    let mut pos = 0;
    while pos + 8 <= len {
        let mut head = [0u8; 24];
        if f.seek(SeekFrom::Start(pos)).is_err() || f.read_exact(&mut head[..8]).is_err() {
            return false;
        }
        let mut size = u32::from_be_bytes(head[..4].try_into().unwrap()) as u64;
        let mut body = 8;
        if size == 1 && f.read_exact(&mut head[8..16]).is_ok() {
            size = u64::from_be_bytes(head[8..16].try_into().unwrap());
            body = 16;
        }
        match &head[4..8] {
            b"c2pa" => return true,
            b"uuid" => {
                let mut id = [0u8; 16];
                if f.seek(SeekFrom::Start(pos + body)).is_ok() && f.read_exact(&mut id).is_ok() && id == C2PA_UUID {
                    return true;
                }
            }
            _ => {}
        }
        if size < 8 {
            return false;
        }
        pos += size;
    }
    false
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
