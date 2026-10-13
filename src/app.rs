use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui::{self, Align, Color32, CornerRadius, Layout, Margin, RichText, Stroke, Ui};

use crate::job::{self, Kind, Options, Outcome, Target};
use crate::report::Report;
use crate::{image_strip, video_strip};

pub const ACCENT: Color32 = Color32::from_rgb(94, 214, 168);
const AI: Color32 = Color32::from_rgb(255, 120, 140);
const WARN: Color32 = Color32::from_rgb(240, 180, 90);
const MUTED: Color32 = Color32::from_rgb(140, 146, 158);
const CARD: Color32 = Color32::from_rgb(32, 35, 42);
const NO_PROOF: &str = "Fehlt eine Kennzeichnung, ist das kein Beweis für ein echtes Bild. \
Viele Plattformen löschen Metadaten beim Hochladen.";

enum Status {
    Waiting,
    Working,
    Done(Outcome),
    Failed(String),
}

enum Event {
    Analyzed(PathBuf, Result<Report, String>),
    Working(PathBuf),
    Cleaned(PathBuf, Result<Outcome, String>),
    Finished,
}

struct Item {
    path: PathBuf,
    kind: Kind,
    report: Option<Result<Report, String>>,
    status: Status,
}

pub struct App {
    items: Vec<Item>,
    opts: Options,
    notice: Option<String>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    cleaning: bool,
    ctx: egui::Context,
}

impl App {
    pub fn new(ctx: egui::Context, files: Vec<PathBuf>) -> Self {
        let opts = Options { target: Target::Beside, folder: None, reencode: false, ffmpeg: video_strip::find_ffmpeg() };
        let (tx, rx) = mpsc::channel();
        let mut app = App { items: Vec::new(), opts, notice: None, tx, rx, cleaning: false, ctx };
        app.add(files);
        app
    }

    fn add(&mut self, paths: Vec<PathBuf>) {
        let (mut found, mut skipped) = (Vec::new(), 0);
        job::collect(paths, &mut found, &mut skipped);
        found.retain(|(p, _)| !self.items.iter().any(|i| &i.path == p));
        for (path, kind) in &found {
            self.items.push(Item { path: path.clone(), kind: *kind, report: None, status: Status::Waiting });
        }
        self.notice = (skipped > 0).then(|| format!("{skipped} Dateien übersprungen, Format wird nicht unterstützt."));
        let (tx, ctx, ffmpeg) = (self.tx.clone(), self.ctx.clone(), self.opts.ffmpeg.clone());
        std::thread::spawn(move || {
            for (path, kind) in found {
                let report = job::analyze(&path, kind, ffmpeg.as_deref());
                let _ = tx.send(Event::Analyzed(path, report));
                ctx.request_repaint();
            }
        });
    }

    fn start(&mut self) {
        let jobs: Vec<(PathBuf, Kind)> = self
            .items
            .iter()
            .filter(|i| matches!(i.status, Status::Waiting | Status::Failed(_)))
            .map(|i| (i.path.clone(), i.kind))
            .collect();
        let (tx, ctx, opts) = (self.tx.clone(), self.ctx.clone(), self.opts.clone());
        self.cleaning = true;
        std::thread::spawn(move || {
            for (path, kind) in jobs {
                let _ = tx.send(Event::Working(path.clone()));
                ctx.request_repaint();
                let result = job::run(&path, kind, &opts);
                let _ = tx.send(Event::Cleaned(path, result));
                ctx.request_repaint();
            }
            let _ = tx.send(Event::Finished);
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            let (path, update): (PathBuf, Box<dyn FnOnce(&mut Item)>) = match event {
                Event::Analyzed(p, r) => (p, Box::new(|i| i.report = Some(r))),
                Event::Working(p) => (p, Box::new(|i| i.status = Status::Working)),
                Event::Cleaned(p, r) => (p, Box::new(|i| i.status = r.map_or_else(Status::Failed, Status::Done))),
                Event::Finished => {
                    self.cleaning = false;
                    continue;
                }
            };
            if let Some(item) = self.items.iter_mut().find(|i| i.path == path) {
                update(item);
            }
        }
    }

    fn pick_files(&mut self) {
        let exts: Vec<&str> = image_strip::EXTENSIONS.iter().chain(video_strip::EXTENSIONS).copied().collect();
        if let Some(files) = rfd::FileDialog::new().add_filter("Bilder und Videos", &exts).pick_files() {
            self.add(files);
        }
    }

    fn pick_folder(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.add(vec![dir]);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.poll();
        let ctx = ui.ctx().clone();
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        if !dropped.is_empty() && !self.cleaning {
            self.add(dropped);
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());

        egui::Panel::top("kopf").frame(bar()).show(ui, |ui| self.header(ui));
        egui::Panel::bottom("fuss").frame(bar()).show(ui, |ui| self.footer(ui));
        let body = egui::Frame::new().fill(Color32::from_rgb(20, 22, 27)).inner_margin(Margin::same(20));
        egui::CentralPanel::default().frame(body).show(ui, |ui| {
            if self.items.is_empty() || hovering {
                self.drop_zone(ui, hovering);
            } else {
                self.list(ui);
            }
        });
    }
}

fn bar() -> egui::Frame {
    egui::Frame::new().fill(Color32::from_rgb(24, 26, 31)).inner_margin(Margin::symmetric(20, 14))
}

fn pill(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(9, 3))
        .show(ui, |ui| ui.label(RichText::new(text).small().strong().color(fg)))
        .response
}

fn small(ui: &mut Ui, text: impl Into<String>, color: Color32) {
    ui.label(RichText::new(text.into()).small().color(color));
}

fn join<'a>(items: impl Iterator<Item = &'a str>) -> String {
    items.collect::<Vec<_>>().join(", ")
}

impl App {
    fn header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Spurlos").size(24.0).strong().color(Color32::WHITE));
                ui.label(RichText::new("Findet Metadaten und KI-Spuren").color(MUTED));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(!self.cleaning, |ui| {
                    if !self.items.is_empty() && ui.button("Liste leeren").clicked() {
                        self.items.clear();
                        self.notice = None;
                    }
                    if ui.button("Ordner hinzufügen").clicked() {
                        self.pick_folder();
                    }
                    if ui.button("Dateien hinzufügen").clicked() {
                        self.pick_files();
                    }
                });
            });
        });
    }

    fn drop_zone(&mut self, ui: &mut Ui, hovering: bool) {
        let stroke = Stroke::new(2.0, if hovering { ACCENT } else { Color32::from_gray(70) });
        egui::Frame::new().stroke(stroke).corner_radius(CornerRadius::same(16)).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 2.0 - 80.0);
                ui.label(RichText::new("+").size(44.0).color(ACCENT));
                let title = if hovering { "Loslassen zum Prüfen" } else { "Dateien oder Ordner hierher ziehen" };
                ui.label(RichText::new(title).size(20.0).color(Color32::WHITE));
                ui.label(RichText::new("Spurlos zeigt sofort, ob KI-Kennzeichnungen, GPS oder Kameradaten drinstecken.").color(MUTED));
                ui.label(RichText::new("JPEG, PNG, WebP, GIF, TIFF, BMP, MP4, MOV, MKV, WebM, AVI").small().color(MUTED));
                ui.add_space(12.0);
                if !hovering && ui.button("Dateien auswählen").clicked() {
                    self.pick_files();
                }
            });
        });
    }

    fn list(&mut self, ui: &mut Ui) {
        let mut remove = None;
        let busy = self.cleaning;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for (n, item) in self.items.iter().enumerate() {
                egui::Frame::new().fill(CARD).corner_radius(CornerRadius::same(10)).inner_margin(Margin::same(12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.horizontal(|ui| {
                        let (badge, color) = match item.kind {
                            Kind::Image => ("BILD", Color32::from_rgb(120, 170, 255)),
                            Kind::Video => ("VIDEO", Color32::from_rgb(230, 160, 90)),
                        };
                        ui.label(RichText::new(badge).small().strong().color(color));
                        let name = item.path.file_name().unwrap_or_default().to_string_lossy();
                        ui.label(RichText::new(name).color(Color32::WHITE)).on_hover_text(item.path.display().to_string());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if !busy && ui.small_button("×").on_hover_text("Aus Liste entfernen").clicked() {
                                remove = Some(n);
                            }
                            status_label(ui, &item.status);
                            match &item.status {
                                Status::Done(o) if !o.before.ai.is_empty() => {
                                    pill(ui, "KI-Kennzeichnung entfernt", Color32::BLACK, ACCENT);
                                }
                                _ => ai_pill(ui, &item.report),
                            }
                        });
                    });
                    details(ui, item);
                });
                ui.add_space(6.0);
            }
        });
        if let Some(n) = remove {
            self.items.remove(n);
        }
    }

    fn footer(&mut self, ui: &mut Ui) {
        ui.add_enabled_ui(!self.cleaning, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Speichern").color(MUTED));
                ui.radio_value(&mut self.opts.target, Target::Beside, "Neben dem Original (_clean)");
                ui.radio_value(&mut self.opts.target, Target::Replace, "Original ersetzen");
                ui.radio_value(&mut self.opts.target, Target::Folder, "In Ordner");
                if self.opts.target == Target::Folder {
                    let label = self.opts.folder.as_ref().map_or("Ordner wählen".into(), |f| f.display().to_string());
                    if ui.button(label).clicked() {
                        self.opts.folder = rfd::FileDialog::new().pick_folder().or(self.opts.folder.take());
                    }
                }
            });
            ui.checkbox(&mut self.opts.reencode, "Videos neu kodieren (H.264 als MP4, läuft überall, dauert länger)");
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            match &self.opts.ffmpeg {
                Some(_) => small(ui, "ffmpeg bereit", MUTED),
                None => small(ui, "ffmpeg nicht gefunden, Videos werden nicht bereinigt", AI),
            }
            let ai = self.items.iter().filter(|i| matches!(&i.report, Some(Ok(r)) if !r.ai.is_empty())).count();
            if ai > 0 {
                small(ui, format!("{ai} von {} mit KI-Kennzeichnung", self.items.len()), AI);
            }
            if let Some(notice) = &self.notice {
                small(ui, notice.clone(), MUTED);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let open = self.items.iter().filter(|i| matches!(i.status, Status::Waiting | Status::Failed(_))).count();
                let done = self.items.iter().filter(|i| matches!(i.status, Status::Done(..))).count();
                let ready = open > 0 && !self.cleaning && (self.opts.target != Target::Folder || self.opts.folder.is_some());
                let text = RichText::new(format!("{open} bereinigen")).strong().color(Color32::BLACK);
                let button = egui::Button::new(text).fill(ACCENT).min_size(egui::vec2(140.0, 34.0));
                if ui.add_enabled(ready, button).clicked() {
                    self.start();
                }
                if self.cleaning {
                    ui.spinner();
                }
                if !self.items.is_empty() {
                    ui.label(RichText::new(format!("{done} von {} fertig", self.items.len())).color(MUTED));
                }
            });
        });
    }
}

fn ai_pill(ui: &mut Ui, report: &Option<Result<Report, String>>) {
    match report {
        None => {
            ui.spinner().on_hover_text("Wird geprüft");
        }
        Some(Err(_)) => {}
        Some(Ok(r)) if !r.ai.is_empty() => {
            pill(ui, "KI erkannt", Color32::from_rgb(40, 10, 16), AI).on_hover_text(join(r.ai.iter().copied()));
        }
        Some(Ok(_)) => {
            pill(ui, "Keine KI-Kennzeichnung", MUTED, Color32::from_rgb(44, 48, 57)).on_hover_text(NO_PROOF);
        }
    }
}

fn status_label(ui: &mut Ui, status: &Status) {
    match status {
        Status::Waiting => small(ui, "Bereit", MUTED),
        Status::Working => {
            ui.spinner();
        }
        Status::Done(o) if o.after.is_clean() => small(ui, "Sauber", ACCENT),
        Status::Done(_) => small(ui, "Prüfen", WARN),
        Status::Failed(_) => small(ui, "Fehler", AI),
    }
}

fn details(ui: &mut Ui, item: &Item) {
    match (&item.status, &item.report) {
        (Status::Done(o), _) => {
            let removed = join(o.before.ai.iter().copied().chain(o.before.items.iter().map(String::as_str)));
            if removed.is_empty() {
                small(ui, "Es waren keine Metadaten enthalten, die Datei wurde trotzdem neu geschrieben", MUTED);
            } else {
                small(ui, format!("Entfernt {removed}"), MUTED);
            }
            if o.after.is_clean() {
                small(ui, "Gegenprüfung der neuen Datei ohne Funde", ACCENT);
            } else {
                let rest = join(o.after.ai.iter().copied().chain(o.after.leftovers().map(String::as_str)));
                small(ui, format!("Gegenprüfung fand noch {rest}"), WARN);
            }
            let name = o.output.file_name().unwrap_or_default().to_string_lossy();
            small(ui, format!("Gespeichert als {name}"), MUTED);
        }
        (Status::Failed(e), _) => small(ui, e.clone(), AI),
        (_, Some(Err(e))) => small(ui, format!("Analyse nicht möglich ({e})"), AI),
        (_, Some(Ok(r))) => {
            if !r.ai.is_empty() {
                small(ui, format!("KI-Quelle {}", join(r.ai.iter().copied())), AI);
            }
            for line in &r.info {
                small(ui, line.clone(), Color32::from_gray(200));
            }
            if r.items.is_empty() {
                small(ui, "Keine Metadaten gefunden", MUTED);
            } else {
                small(ui, format!("Enthält {}", join(r.items.iter().map(String::as_str))), MUTED);
            }
        }
        _ => {}
    }
}
