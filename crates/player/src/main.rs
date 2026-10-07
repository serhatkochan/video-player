#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod diagnostics;
mod host;
mod i18n;
mod mpv;
mod updates;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let mut file = None;
    let mut smoke_report = None;
    for arg in std::env::args_os().skip(1) {
        if arg == "--diagnose" {
            println!(
                "{}",
                serde_json::to_string_pretty(&diagnostics::check(i18n::system_language())).unwrap()
            );
            return Ok(());
        }
        if arg == "--version" {
            println!("Video Player {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        if let Some(path) = arg
            .to_str()
            .and_then(|arg| arg.strip_prefix("--smoke-report="))
        {
            smoke_report = Some(PathBuf::from(path));
        } else if file.is_none() {
            file = Some(PathBuf::from(arg));
        }
    }
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/video-player.png"))
        .expect("Bundled Video Player icon must be a valid PNG");
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Video Player")
            .with_icon(icon)
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([640.0, 420.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Video Player",
        options,
        Box::new(move |cc| Ok(Box::new(app::PlayerApp::new(cc, file, smoke_report)))),
    )
}
