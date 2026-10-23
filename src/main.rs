// In release builds on Windows, do not open a console window.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod config;
mod errcodes;
mod i18n;
mod parser;

use eframe::egui;

fn main() -> eframe::Result {
    // Optional: path to a log file as the first argument (e.g. .log file association).
    let path = std::env::args().nth(1);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 850.0])
            .with_min_inner_size([700.0, 400.0])
            .with_title("RusTrace")
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "RusTrace",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, path)))),
    )
}
