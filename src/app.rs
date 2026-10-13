use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, Align, Color32, CornerRadius, Layout, Margin, RichText, Stroke, Ui};

use crate::job::{self, Kind, Options, Target};
use crate::{image_strip, video_strip};

pub const ACCENT: Color32 = Color32::from_rgb(94, 214, 168);
const ERROR: Color32 = Color32::from_rgb(240, 110, 110);
const MUTED: Color32 = Color32::from_rgb(140, 146, 158);
const CARD: Color32 = Color32::from_rgb(32, 35, 42);

enum Status {
    Waiting,
    Working,
    Done(PathBuf, BTreeSet<String>),
    Failed(String),
}

struct Item {
    path: PathBuf,
    kind: Kind,
    status: Status,
}

pub struct App {
    items: Vec<Item>,
    opts: Options,
    notice: Option<String>,
    rx: Option<Receiver<(usize, Status)>>,
}

impl App {
    pub fn new(files: Vec<PathBuf>) -> Self {
        let opts = Options { target: Target::Beside, folder: None, reencode: false, ffmpeg: video_strip::find_ffmpeg() };
        let mut app = App { items: Vec::new(), opts, notice: None, rx: None };
        app.add(files);
        app
    }

    fn busy(&self) -> bool {
        self.rx.is_some()
    }

    fn add(&mut self, paths: Vec<PathBuf>) {
        let (mut found, mut skipped) = (Vec::new(), 0);
        job::collect(paths, &mut found, &mut skipped);
        for (path, kind) in found {
            if !self.items.iter().any(|i| i.path == path) {
                self.items.push(Item { path, kind, status: Status::Waiting });
            }
        }
        self.notice = (skipped > 0).then(|| format!("{skipped} Dateien übersprungen, Format wird nicht unterstützt."));
    }

    fn start(&mut self, ctx: &egui::Context) {
        let jobs: Vec<(usize, PathBuf, Kind)> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| matches!(i.status, Status::Waiting | Status::Failed(_)))
            .map(|(n, i)| (n, i.path.clone(), i.kind))
            .collect();
        let (tx, rx) = mpsc::channel();
        let (opts, ctx) = (self.opts.clone(), ctx.clone());
        std::thread::spawn(move || {
            for (n, path, kind) in jobs {
                let _ = tx.send((n, Status::Working));
                ctx.request_repaint();
                let status = match job::run(&path, kind, &opts) {
                    Ok((out, removed)) => Status::Done(out, removed),
                    Err(e) => Status::Failed(e),
                };
                let _ = tx.send((n, status));
                ctx.request_repaint();
            }
        });
        self.rx = Some(rx);
    }

    fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        loop {
            match rx.try_recv() {
                Ok((n, status)) => self.items[n].status = status,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        self.rx = None;
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
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if !dropped.is_empty() && !self.busy() {
            self.add(dropped);
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());

        egui::Panel::top("kopf").frame(bar()).show(ui, |ui| self.header(ui));
        egui::Panel::bottom("fuss").frame(bar()).show(ui, |ui| self.footer(ui, &ctx));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(20, 22, 27)).inner_margin(Margin::same(20))).show(ui, |ui| {
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

impl App {
    fn header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Spurlos").size(24.0).strong().color(Color32::WHITE));
                ui.label(RichText::new("Entfernt alle Metadaten aus Bildern und Videos").color(MUTED));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(!self.busy(), |ui| {
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
                ui.add_space(ui.available_height() / 2.0 - 70.0);
                ui.label(RichText::new("+").size(44.0).color(ACCENT));
                let title = if hovering { "Loslassen zum Hinzufügen" } else { "Dateien oder Ordner hierher ziehen" };
                ui.label(RichText::new(title).size(20.0).color(Color32::WHITE));
                ui.label(RichText::new("JPEG, PNG, WebP, GIF, TIFF, BMP, MP4, MOV, MKV, WebM, AVI").color(MUTED));
                ui.add_space(12.0);
                if !hovering && ui.button("Dateien auswählen").clicked() {
                    self.pick_files();
                }
            });
        });
    }

    fn list(&mut self, ui: &mut Ui) {
        let mut remove = None;
        let busy = self.busy();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for (n, item) in self.items.iter().enumerate() {
                egui::Frame::new().fill(CARD).corner_radius(CornerRadius::same(10)).inner_margin(Margin::same(12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
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
                        });
                    });
                    details(ui, &item.status);
                });
                ui.add_space(6.0);
            }
        });
        if let Some(n) = remove {
            self.items.remove(n);
        }
    }

    fn footer(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.add_enabled_ui(!self.busy(), |ui| {
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
                Some(_) => ui.label(RichText::new("ffmpeg bereit").small().color(MUTED)),
                None => ui.label(RichText::new("ffmpeg nicht gefunden, Videos werden nicht bereinigt").small().color(ERROR)),
            };
            if let Some(notice) = &self.notice {
                ui.label(RichText::new(notice).small().color(MUTED));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let open = self.items.iter().filter(|i| matches!(i.status, Status::Waiting | Status::Failed(_))).count();
                let done = self.items.iter().filter(|i| matches!(i.status, Status::Done(..))).count();
                let ready = open > 0 && !self.busy() && (self.opts.target != Target::Folder || self.opts.folder.is_some());
                let text = RichText::new(format!("{open} bereinigen")).strong().color(Color32::BLACK);
                let button = egui::Button::new(text).fill(ACCENT).min_size(egui::vec2(140.0, 34.0));
                if ui.add_enabled(ready, button).clicked() {
                    self.start(ctx);
                }
                if self.busy() {
                    ui.spinner();
                }
                if !self.items.is_empty() {
                    ui.label(RichText::new(format!("{done} von {} fertig", self.items.len())).color(MUTED));
                }
            });
        });
    }
}

fn status_label(ui: &mut Ui, status: &Status) {
    match status {
        Status::Waiting => ui.label(RichText::new("Bereit").color(MUTED)),
        Status::Working => ui.spinner(),
        Status::Done(..) => ui.label(RichText::new("Bereinigt").color(ACCENT)),
        Status::Failed(_) => ui.label(RichText::new("Fehler").color(ERROR)),
    };
}

fn details(ui: &mut Ui, status: &Status) {
    match status {
        Status::Done(out, removed) => {
            let what = if removed.is_empty() {
                "Keine Metadaten gefunden, Datei trotzdem neu geschrieben".to_string()
            } else {
                format!("Entfernt {}", removed.iter().cloned().collect::<Vec<_>>().join(", "))
            };
            ui.label(RichText::new(what).small().color(MUTED));
            let name = out.file_name().unwrap_or_default().to_string_lossy();
            ui.label(RichText::new(format!("Gespeichert als {name}")).small().color(MUTED));
        }
        Status::Failed(e) => {
            ui.label(RichText::new(e).small().color(ERROR));
        }
        _ => {}
    }
}
