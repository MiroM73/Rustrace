//! Error / warning / search colors window.

use crate::app::*;

impl App {
    pub(in crate::app) fn colors_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_colors;
        let mut changed = false;
        let mut reload = false;
        egui::Window::new(t!("colors.title"))
            .id(egui::Id::unique("colors_window"))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                changed |= ui
                    .checkbox(&mut self.cfg.color_by_severity, t!("colors.by_severity"))
                    .changed();
                if ui
                    .checkbox(&mut self.cfg.keyword_severity, t!("colors.keyword_severity"))
                    .changed()
                {
                    changed = true;
                    reload = true;
                }
                ui.separator();
                let edit_dark = self.edit_dark;
                theme_selector(ui, &mut self.edit_dark);
                ui.add_space(4.0);
                egui::Grid::new("sev_colors").spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.strong("");
                    ui.strong(t!("rules.col.text"));
                    ui.strong(t!("rules.col.background"));
                    ui.strong(t!("rules.col.preview"));
                    ui.end_row();
                    for (key, fg, bg) in self.cfg.palette_mut(edit_dark) {
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
                        self.cfg.reset_palette(edit_dark);
                        changed = true;
                    }
                    let other = theme_name(!edit_dark);
                    if ui
                        .button(format!("⇄ {}", t!("colors.copy_from", theme = other)))
                        .clicked()
                    {
                        let src = self.cfg.palette(!edit_dark);
                        let [e, w, s] = self.cfg.palette_mut(edit_dark);
                        for ((_, fg, bg), (sf, sb)) in
                            [e, w, s].into_iter().zip([src.error, src.warning, src.search])
                        {
                            *fg = sf;
                            *bg = sb;
                        }
                        changed = true;
                    }
                });
            });
        self.show_colors = open;
        if changed {
            self.cfg_dirty = true;
        }
        if reload {
            self.reload_full();
        }
    }
}
