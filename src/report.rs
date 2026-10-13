//! Ergebnis einer Analyse. Gilt für die Originaldatei vor dem Bereinigen und
//! für die neue Datei bei der Gegenprüfung.

use std::collections::BTreeSet;

/// Angaben, die jede Datei eines Formats trägt und die nichts verraten.
const HARMLESS: &[&str] = &["JFIF-Kopf", "major_brand", "minor_version", "compatible_brands", "handler_name", "vendor_id"];

#[derive(Clone, Default)]
pub struct Report {
    /// Gefundene Arten von Metadaten, etwa EXIF oder XMP.
    pub items: BTreeSet<String>,
    /// Erkannte KI-Quellen, etwa OpenAI oder Midjourney.
    pub ai: BTreeSet<&'static str>,
    /// Lesbare Einzelheiten wie Kamera, Datum oder GPS-Position.
    pub info: Vec<String>,
}

impl Report {
    pub fn note(&mut self, label: &str, data: &[u8]) {
        self.items.insert(label.into());
        self.ai.extend(crate::ai::scan(data));
    }

    pub fn detail(&mut self, label: &str, value: &str) {
        let value: String = value.trim().trim_matches('"').chars().take(90).collect();
        let line = format!("{label} {value}");
        if !value.is_empty() && !self.info.contains(&line) {
            self.info.push(line);
        }
    }

    pub fn is_clean(&self) -> bool {
        self.ai.is_empty() && self.info.is_empty() && self.leftovers().next().is_none()
    }

    pub fn leftovers(&self) -> impl Iterator<Item = &String> {
        self.items.iter().filter(|i| !HARMLESS.contains(&i.as_str()))
    }
}
