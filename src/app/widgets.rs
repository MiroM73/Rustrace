//! Small UI helpers shared by several windows.

use crate::app::*;

/// Color picker button for an RGBA array; returns true when changed.
pub(in crate::app) fn color_button(ui: &mut egui::Ui, color: &mut [u8; 4]) -> bool {
    let mut c = c32(*color);
    let changed = ui.color_edit_button_srgba(&mut c).changed();
    if changed {
        *color = c.to_srgba_unmultiplied();
    }
    changed
}

/// Sample text drawn on the background of the given theme.
pub(in crate::app) fn preview(
    ui: &mut egui::Ui,
    dark: bool,
    fg: Option<[u8; 4]>,
    bg: Option<[u8; 4]>,
) {
    let v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let mut text = RichText::new(format!(" {} ", t!("rules.preview_text")))
        .color(fg.map(c32).unwrap_or(v.text_color()));
    if let Some(bg) = bg {
        text = text.background_color(c32(bg));
    }
    egui::Frame::new()
        .fill(v.extreme_bg_color)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .corner_radius(3.0)
        .show(ui, |ui| {
            ui.label(text);
        });
}

pub(in crate::app) fn theme_name(dark: bool) -> String {
    if dark {
        t!("menu.view.theme.dark")
    } else {
        t!("menu.view.theme.light")
    }
}

/// "Colors for theme: [Light] [Dark]" – returns true when the selection changed.
pub(in crate::app) fn theme_selector(ui: &mut egui::Ui, edit_dark: &mut bool) -> bool {
    let before = *edit_dark;
    ui.horizontal(|ui| {
        ui.label(t!("colors.for_theme"));
        ui.selectable_value(edit_dark, false, format!("☀ {}", theme_name(false)));
        ui.selectable_value(edit_dark, true, format!("🌙 {}", theme_name(true)));
    });
    before != *edit_dark
}

/// Localizes modifier key names in a shortcut ("Ctrl+O" → "Strg+O" in German).
pub(in crate::app) fn kb(shortcut: &str) -> String {
    shortcut
        .replace("Ctrl", &t!("key.ctrl"))
        .replace("Shift", &t!("key.shift"))
}

pub(in crate::app) fn scope_label(s: HighlightScope) -> String {
    match s {
        HighlightScope::Row => t!("rules.scope.row"),
        HighlightScope::Text => t!("rules.scope.text"),
    }
}
