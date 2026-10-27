//! Help → Set as default app (Windows only).

use crate::app::*;

impl App {
    /// Help → Set as default app: registers RusTrace for .log / .lo_ files (per user).
    pub(in crate::app) fn assoc_window(&mut self, ctx: &egui::Context) {
        let Some(info) = &self.assoc else {
            return;
        };
        let mut open = true;
        let mut action = None;
        let mut settings_err = None;
        egui::Window::new(t!("assoc.title"))
            .id(egui::Id::unique("assoc_window"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.add(egui::Label::new(t!("assoc.intro")).wrap());
                ui.add_space(6.0);
                egui::Grid::new("assoc_grid")
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        for (ext, status) in &info.exts {
                            ui.monospace(*ext);
                            match status {
                                ExtStatus::RusTrace => ui.colored_label(
                                    Color32::from_rgb(60, 170, 80),
                                    t!("assoc.ext.rustrace"),
                                ),
                                ExtStatus::Other(prog) => {
                                    ui.colored_label(ORANGE, t!("assoc.ext.other", prog = prog))
                                }
                                ExtStatus::None => ui.label(t!("assoc.ext.none")),
                            };
                            ui.end_row();
                        }
                    });
                if let Some(cmd) = &info.command
                    && !info.this_exe
                {
                    ui.add_space(4.0);
                    ui.colored_label(ORANGE, t!("assoc.other_copy", command = cmd));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(format!("✔ {}", t!("assoc.register"))).clicked() {
                        action = Some(true);
                    }
                    if ui
                        .add_enabled(
                            info.command.is_some(),
                            egui::Button::new(t!("assoc.unregister")),
                        )
                        .clicked()
                    {
                        action = Some(false);
                    }
                });
                if info
                    .exts
                    .iter()
                    .any(|(_, s)| matches!(s, ExtStatus::Other(_)))
                {
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(t!("assoc.choice_hint")).wrap());
                    if ui
                        .button(format!("⚙ {}", t!("assoc.open_settings")))
                        .clicked()
                        && let Err(e) = fileassoc::open_default_apps_settings()
                    {
                        settings_err = Some(e.to_string());
                    }
                }
                ui.add_space(4.0);
                ui.weak(t!("assoc.move_hint"));
            });
        match action {
            Some(true) => {
                let res = fileassoc::register(&t!("assoc.type_name"));
                self.info(match res {
                    Ok(()) => t!("assoc.registered"),
                    Err(e) => t!("assoc.failed", error = e),
                });
            }
            Some(false) => {
                let res = fileassoc::unregister();
                self.info(match res {
                    Ok(()) => t!("assoc.unregistered"),
                    Err(e) => t!("assoc.failed", error = e),
                });
            }
            None => {}
        }
        if let Some(e) = settings_err {
            self.info(e);
        }
        self.assoc = if !open {
            None
        } else if action.is_some() {
            Some(fileassoc::info())
        } else {
            self.assoc.take()
        };
    }
}
