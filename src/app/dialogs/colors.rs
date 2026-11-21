//! Error / warning / search colors window.

use crate::app::*;

impl App {
    pub(in crate::app) fn colors_window(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        let mut reload = false;
        let spec = ToolSpec::new(t!("colors.title"), [480.0, 340.0], [380.0, 260.0]);
        self.show_tool(
            ctx,
            |a| &mut a.colors_win,
            spec,
            |app, ui, _keep| {
                changed |= ui
                    .checkbox(&mut app.cfg.color_by_severity, t!("colors.by_severity"))
                    .changed();
                if ui
                    .checkbox(&mut app.cfg.keyword_severity, t!("colors.keyword_severity"))
                    .changed()
                {
                    changed = true;
                    reload = true;
                }
                ui.separator();
                let edit_dark = app.edit_dark;
                theme_selector(ui, &mut app.edit_dark);
                ui.add_space(4.0);
                egui::Grid::new("sev_colors")
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong("");
                        ui.strong(t!("rules.col.text"));
                        ui.strong(t!("rules.col.background"));
                        ui.strong(t!("rules.col.preview"));
                        ui.end_row();
                        for (key, fg, bg) in app.cfg.palette_mut(edit_dark) {
                            ui.label(t!(key));
                            changed |= color_button(ui, fg);
                            changed |= color_button(ui, bg);
                            preview(ui, edit_dark, Some(*fg), Some(*bg));
                            ui.end_row();
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t!("colors.restore_defaults")).clicked() {
                        app.cfg.reset_palette(edit_dark);
                        changed = true;
                    }
                    let other = theme_name(!edit_dark);
                    if ui
                        .button(format!("⇄ {}", t!("colors.copy_from", theme = other)))
                        .clicked()
                    {
                        let src = app.cfg.palette(!edit_dark);
                        app.cfg.set_palette(edit_dark, &src);
                        changed = true;
                    }
                });
            },
        );
        if changed {
            self.cfg_dirty = true;
        }
        if reload {
            self.reload_full();
        }
    }
}
