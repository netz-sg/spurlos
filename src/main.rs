#![cfg_attr(windows, windows_subsystem = "windows")]

mod ai;
mod app;
mod image_strip;
mod job;
mod report;
mod video_strip;

use std::path::PathBuf;

use eframe::egui::{self, Color32, CornerRadius, FontId, TextStyle, Visuals};

fn main() -> eframe::Result {
    let mut args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("-psn")).collect();
    if args.first().is_some_and(|a| a == "--cli") {
        args.remove(0);
        cli(args);
        return Ok(());
    }
    let files: Vec<PathBuf> = args.into_iter().map(PathBuf::from).collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Spurlos")
            .with_inner_size([820.0, 620.0])
            .with_min_inner_size([600.0, 440.0])
            .with_drag_and_drop(true)
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native(
        "Spurlos",
        options,
        Box::new(|cc| {
            style(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc.egui_ctx.clone(), files)))
        }),
    )
}

fn style(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(20, 22, 27);
    visuals.window_fill = visuals.panel_fill;
    visuals.selection.bg_fill = app::ACCENT.gamma_multiply(0.5);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 56, 66);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 43, 51);
    for w in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active] {
        w.corner_radius = CornerRadius::same(8);
    }
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.text_styles.insert(TextStyle::Body, FontId::proportional(15.0));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(15.0));
        style.text_styles.insert(TextStyle::Small, FontId::proportional(12.5));
    });
}

/// Ohne Fenster. Mit --check wird nur analysiert, sonst bereinigt und neben
/// dem Original abgelegt.
fn cli(args: Vec<String>) {
    let flag = |f: &str| args.iter().any(|a| a == f);
    let (check, reencode) = (flag("--check"), flag("--reencode"));
    let paths = args.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    let opts = job::Options { target: job::Target::Beside, folder: None, reencode, ffmpeg: video_strip::find_ffmpeg() };
    let (mut found, mut skipped) = (Vec::new(), 0);
    job::collect(paths, &mut found, &mut skipped);
    for (path, kind) in found {
        println!("{}", path.display());
        let result = if check {
            job::analyze(&path, kind, opts.ffmpeg.as_deref()).map(|r| print_report("  ", &r))
        } else {
            job::run(&path, kind, &opts).map(|o| {
                print_report("  vorher ", &o.before);
                println!("  neu    {}", o.output.display());
                print_report("  danach ", &o.after);
                println!("  {}", if o.after.is_clean() { "SAUBER" } else { "NICHT SAUBER" });
            })
        };
        if let Err(e) = result {
            println!("  FEHLER {e}");
        }
    }
}

fn print_report(prefix: &str, r: &report::Report) {
    let ai = if r.ai.is_empty() { "keine".to_string() } else { Vec::from_iter(r.ai.iter().copied()).join(", ") };
    println!("{prefix}KI {ai}");
    println!("{prefix}Daten {}", Vec::from_iter(r.items.iter().cloned()).join(", "));
    for line in &r.info {
        println!("{prefix}  {line}");
    }
}
