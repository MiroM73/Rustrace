//! Error code lookup window (Ctrl+E).

use crate::app::*;

impl App {
    pub(in crate::app) fn lookup_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_lookup;
        egui::Window::new(format!("🔎 {}", t!("lookup.title")))
            .id(egui::Id::unique("lookup_window"))
            .open(&mut open)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t!("lookup.code"));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.lookup_input)
                            .hint_text("0x80070005, 80070005, -2147024891, 1603 …")
                            .desired_width(280.0)
                            .font(TextStyle::Monospace),
                    );
                    if self.lookup_focus {
                        r.request_focus();
                        self.lookup_focus = false;
                    }
                    if !self.lookup_input.is_empty() && ui.small_button("✖").clicked() {
                        self.lookup_input.clear();
                    }
                });
                ui.separator();

                let input = self.lookup_input.trim().to_owned();
                if input.is_empty() {
                    ui.weak(t!("lookup.hint"));
                    return;
                }
                if self.lookup_cache.as_ref().is_none_or(|(k, _)| *k != input) {
                    let info = errcodes::parse_code(&input).map(errcodes::lookup);
                    self.lookup_cache = Some((input.clone(), info));
                }
                let Some((_, Some(info))) = &self.lookup_cache else {
                    ui.colored_label(RED, t!("lookup.invalid"));
                    return;
                };
                egui::Grid::new("code_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(t!("lookup.hex"));
                        ui.monospace(info.hex());
                        ui.end_row();
                        ui.label(t!("lookup.decimal"));
                        ui.monospace(t!(
                            "lookup.decimal.value",
                            signed = info.signed(),
                            unsigned = info.hresult
                        ));
                        ui.end_row();
                        if let Some(w) = info.win32_code() {
                            ui.label(t!("lookup.win32"));
                            ui.monospace(format!("{w}  (0x{w:X})"));
                            ui.end_row();
                        }
                        ui.label(t!("lookup.facility"));
                        ui.label(&info.facility);
                        ui.end_row();
                    });
                ui.separator();
                if info.descriptions.is_empty() {
                    ui.weak(t!("lookup.no_description"));
                    if !cfg!(windows) {
                        ui.weak(t!("lookup.windows_only"));
                    }
                }
                for (src, d) in &info.descriptions {
                    ui.strong(src);
                    ui.label(d);
                    ui.add_space(4.0);
                }
                if let Some((_, d)) = info.descriptions.first()
                    && ui.button(format!("📋 {}", t!("lookup.copy"))).clicked()
                {
                    ui.ctx().copy_text(format!("{} – {d}", info.hex()));
                }
            });
        self.show_lookup = open;
    }
}
