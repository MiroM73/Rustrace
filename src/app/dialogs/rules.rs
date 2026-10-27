//! Highlight rules window (Ctrl+H).

use crate::app::*;

impl App {
    pub(in crate::app) fn rules_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_rules;
        let mut changed = false;
        egui::Window::new(format!("🎨 {}", t!("rules.title")))
            .id(egui::Id::unique("rules_window"))
            .open(&mut open)
            .default_width(1150.0)
            .min_width(900.0)
            .default_height(420.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(t!("rules.help"));
                ui.separator();
                let edit_dark = self.edit_dark;
                ui.horizontal(|ui| {
                    theme_selector(ui, &mut self.edit_dark);
                    let other = theme_name(!edit_dark);
                    if ui
                        .button(format!("⇄ {}", t!("colors.copy_from", theme = other)))
                        .on_hover_text(t!("colors.copy_from.tip"))
                        .clicked()
                    {
                        for r in &mut self.cfg.rules {
                            r.copy_colors(!edit_dark);
                        }
                        changed = true;
                    }
                });
                ui.separator();
                let mut act: Option<(usize, i32)> = None; // (index, -1 up / 1 down / 0 delete)
                egui::ScrollArea::both()
                    .max_height(ui.available_height() - 40.0)
                    .show(ui, |ui| {
                        egui::Grid::new("rules_grid")
                            .striped(true)
                            .spacing([8.0, 6.0])
                            .show(ui, |ui| {
                                for h in [
                                    t!("rules.col.on"),
                                    t!("rules.col.name"),
                                    t!("rules.col.pattern"),
                                    String::new(),
                                    "Regex".to_owned(),
                                    "Aa".to_owned(),
                                    t!("rules.col.scope"),
                                    t!("rules.col.component"),
                                    t!("rules.col.text"),
                                    t!("rules.col.background"),
                                    t!("rules.col.preview"),
                                    String::new(),
                                ] {
                                    ui.strong(h);
                                }
                                ui.end_row();

                                let n = self.cfg.rules.len();
                                for i in 0..n {
                                    let err = self.rule_errors.get(i).cloned().flatten();
                                    let r = &mut self.cfg.rules[i];
                                    ui.push_id(i, |ui| {
                                        changed |= ui.checkbox(&mut r.enabled, "").changed();
                                    });
                                    changed |= ui
                                        .add_sized(
                                            [150.0, 20.0],
                                            egui::TextEdit::singleline(&mut r.name),
                                        )
                                        .changed();
                                    changed |= ui
                                        .add_sized(
                                            [300.0, 20.0],
                                            egui::TextEdit::singleline(&mut r.pattern)
                                                .font(TextStyle::Monospace)
                                                .hint_text(t!("rules.pattern.hint")),
                                        )
                                        .changed();
                                    if let Some(e) = err {
                                        ui.colored_label(RED, "⚠").on_hover_text(e);
                                    } else {
                                        ui.label("");
                                    }
                                    changed |= ui.checkbox(&mut r.is_regex, "").changed();
                                    changed |= ui
                                        .checkbox(&mut r.case_sensitive, "")
                                        .on_hover_text(t!("search.case.tip"))
                                        .changed();
                                    let before = r.scope;
                                    egui::ComboBox::from_id_salt(("scope", i))
                                        .selected_text(scope_label(r.scope))
                                        .width(110.0)
                                        .show_ui(ui, |ui| {
                                            for s in [HighlightScope::Row, HighlightScope::Text] {
                                                ui.selectable_value(
                                                    &mut r.scope,
                                                    s,
                                                    scope_label(s),
                                                );
                                            }
                                        });
                                    changed |= before != r.scope;
                                    let comp_enabled = r.scope == HighlightScope::Row;
                                    changed |= ui
                                        .add_enabled(
                                            comp_enabled,
                                            egui::Checkbox::new(&mut r.match_component, ""),
                                        )
                                        .on_hover_text(t!("rules.component.tip"))
                                        .changed();
                                    ui.horizontal(|ui| {
                                        changed |= ui.checkbox(&mut r.use_fg, "").changed();
                                        let (fg, _) = r.colors_mut(edit_dark);
                                        changed |= color_button(ui, fg);
                                    });
                                    ui.horizontal(|ui| {
                                        changed |= ui.checkbox(&mut r.use_bg, "").changed();
                                        let (_, bg) = r.colors_mut(edit_dark);
                                        changed |= color_button(ui, bg);
                                    });
                                    let (fg, bg) = r.colors(edit_dark);
                                    preview(
                                        ui,
                                        edit_dark,
                                        r.use_fg.then_some(fg),
                                        r.use_bg.then_some(bg),
                                    );
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add_enabled(i > 0, egui::Button::new("⬆").small())
                                            .clicked()
                                        {
                                            act = Some((i, -1));
                                        }
                                        if ui
                                            .add_enabled(i + 1 < n, egui::Button::new("⬇").small())
                                            .clicked()
                                        {
                                            act = Some((i, 1));
                                        }
                                        if ui
                                            .small_button("🗑")
                                            .on_hover_text(t!("rules.delete"))
                                            .clicked()
                                        {
                                            act = Some((i, 0));
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                    });
                if let Some((i, d)) = act {
                    match d {
                        -1 => self.cfg.rules.swap(i, i - 1),
                        1 => self.cfg.rules.swap(i, i + 1),
                        _ => {
                            self.cfg.rules.remove(i);
                        }
                    }
                    changed = true;
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(format!("➕ {}", t!("rules.add"))).clicked() {
                        self.cfg.rules.push(HighlightRule::default());
                        changed = true;
                    }
                    if ui.button(format!("📥 {}", t!("rules.import"))).clicked()
                        && let Some(p) = rfd::FileDialog::new()
                            .add_filter("JSON", &["json"])
                            .pick_file()
                    {
                        match std::fs::read_to_string(&p)
                            .map_err(|e| e.to_string())
                            .and_then(|s| {
                                serde_json::from_str::<Vec<HighlightRule>>(&s)
                                    .map_err(|e| e.to_string())
                            }) {
                            Ok(rules) => {
                                self.cfg.rules.extend(rules);
                                self.cfg.fill_missing_dark();
                                changed = true;
                            }
                            Err(e) => self.status_error = Some(t!("error.import", error = e)),
                        }
                    }
                    if ui.button(format!("📤 {}", t!("rules.export"))).clicked()
                        && let Some(p) = rfd::FileDialog::new()
                            .add_filter("JSON", &["json"])
                            .set_file_name("rustrace-rules.json")
                            .save_file()
                    {
                        let json =
                            serde_json::to_string_pretty(&self.cfg.rules).unwrap_or_default();
                        if let Err(e) = std::fs::write(&p, json) {
                            self.status_error = Some(t!("error.export", error = e));
                        }
                    }
                    if ui.button(t!("rules.restore_defaults")).clicked() {
                        self.cfg.rules = Config::default_rules();
                        changed = true;
                    }
                });
            });
        self.show_rules = open;
        if changed {
            self.recompile_rules();
            self.cfg_dirty = true;
        }
    }
}
