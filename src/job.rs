//! Verarbeitet eine Datei und legt das Ergebnis am gewünschten Ort ab.
//! Geschrieben wird zuerst in eine versteckte Zwischendatei, damit bei einem
//! Fehler nie eine halbe Datei oder ein beschädigtes Original zurückbleibt.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::{image_strip, video_strip};

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Image,
    Video,
}

impl Kind {
    pub fn of(path: &Path) -> Option<Kind> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        if image_strip::EXTENSIONS.contains(&ext.as_str()) {
            Some(Kind::Image)
        } else if video_strip::EXTENSIONS.contains(&ext.as_str()) {
            Some(Kind::Video)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Target {
    Beside,
    Replace,
    Folder,
}

#[derive(Clone)]
pub struct Options {
    pub target: Target,
    pub folder: Option<PathBuf>,
    pub reencode: bool,
    pub ffmpeg: Option<PathBuf>,
}

pub fn run(input: &Path, kind: Kind, opts: &Options) -> Result<(PathBuf, BTreeSet<String>), String> {
    let output = output_path(input, kind, opts)?;
    let dir = output.parent().ok_or("Ungültiger Zielpfad")?;
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or_default();
    let tmp = dir.join(format!(".spurlos-{}.{ext}", std::process::id()));
    let result = match kind {
        Kind::Image => image_strip::strip(input, &tmp),
        Kind::Video => {
            let ffmpeg = opts.ffmpeg.as_deref().ok_or("ffmpeg fehlt, Videos sind nicht möglich")?;
            video_strip::strip(ffmpeg, input, &tmp, opts.reencode)
        }
    };
    let removed = result.inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    std::fs::rename(&tmp, &output).map_err(|e| e.to_string())?;
    if opts.target == Target::Replace && output != input {
        std::fs::remove_file(input).map_err(|e| e.to_string())?;
    }
    Ok((output, removed))
}

fn output_path(input: &Path, kind: Kind, opts: &Options) -> Result<PathBuf, String> {
    let stem = input.file_stem().and_then(|s| s.to_str()).ok_or("Ungültiger Dateiname")?;
    let original = input.extension().and_then(|e| e.to_str()).unwrap_or_default();
    let ext = if kind == Kind::Video && opts.reencode { "mp4" } else { original };
    let dir = input.parent().unwrap_or(Path::new("."));
    Ok(match opts.target {
        Target::Replace if ext == original => input.to_path_buf(),
        Target::Replace => free_name(dir, stem, ext),
        Target::Beside => free_name(dir, &format!("{stem}_clean"), ext),
        Target::Folder => {
            let folder = opts.folder.as_deref().ok_or("Kein Zielordner gewählt")?;
            free_name(folder, stem, ext)
        }
    })
}

fn free_name(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    path
}

pub fn collect(paths: Vec<PathBuf>, out: &mut Vec<(PathBuf, Kind)>, skipped: &mut usize) {
    for path in paths {
        if path.is_dir() {
            let mut children: Vec<PathBuf> = std::fs::read_dir(&path)
                .map(|rd| rd.flatten().map(|e| e.path()).collect())
                .unwrap_or_default();
            children.retain(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')));
            children.sort();
            collect(children, out, skipped);
        } else if let Some(kind) = Kind::of(&path) {
            out.push((path, kind));
        } else {
            *skipped += 1;
        }
    }
}
