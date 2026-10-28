//! Highlight rules window (Ctrl+H).

use crate::app::*;

/// Width of the rule name field.
const NAME_W: f32 = 140.0;
/// Width of the pattern field; a longer pattern is shown in the field's tooltip (#48).
const PATTERN_W: f32 = 250.0;
/// Preferred window width: the table of the rules fits at this width in all languages (#48).
const WINDOW_W: f32 = 1240.0;

/// Default width of the rules window: wide enough for the table, but never wider than the
/// application window (minus a small margin) and never under the window's minimum width.
fn default_window_width(screen_w: f32) -> f32 {
    WINDOW_W.min(screen_w - 40.0).max(MIN_WINDOW_W)
}
const MIN_WINDOW_W: f32 = 600.0;
/// Height of the list of rules to export (#51).
const PICK_H: f32 = 140.0;

/// Moves rule `i` up (`d` = -1) or down (1), or deletes it (0) – also used for the export choice.
fn move_rule<T>(v: &mut Vec<T>, i: usize, d: i32) {
    match d {
        -1 => v.swap(i, i - 1),
        1 => v.swap(i, i + 1),
        _ => {
            v.remove(i);
        }
    }
}

/// The rules ticked for the export, in their order (#51).
fn picked_rules(rules: &[HighlightRule], pick: &[bool]) -> Vec<HighlightRule> {
    rules
        .iter()
        .zip(pick)
        .filter(|(_, p)| **p)
        .map(|(r, _)| r.clone())
        .collect()
}

impl App {
    pub(in crate::app) fn rules_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_rules;
        let just_opened = open && !self.rules_was_open;
        let mut changed = false;
        let screen_w = ctx.content_rect().width();
        egui::Window::new(format!("🎨 {}", t!("rules.title")))
            .id(egui::Id::unique("rules_window"))
            .open(&mut open)
            .default_width(default_window_width(screen_w))
            .min_width(MIN_WINDOW_W)
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
                // A narrow window scrolls the table horizontally; the solid scroll bar style keeps
                // the bars visible whenever they are needed (the default floating bars only show
                // on hover, so the cut-off columns went unnoticed, #48).
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                // Room below the table: the buttons, the export choice, the import result.
                let below =
                    40.0 + if self.rules_export_pick.is_some() {
                        PICK_H + 60.0
                    } else {
                        0.0
                    } + if self.rules_msg.is_some() { 20.0 } else { 0.0 };
                let mut area = egui::ScrollArea::both()
                    .id_salt("rules_scroll")
                    .max_height(ui.available_height() - below);
                if just_opened {
                    // Always start with the first columns (On, Name) in view.
                    area = area.horizontal_scroll_offset(0.0);
                }
                area.show(ui, |ui| {
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
                                let name = ui.add_sized(
                                    [NAME_W, 20.0],
                                    egui::TextEdit::singleline(&mut r.name),
                                );
                                changed |= name.changed();
                                if !r.name.is_empty() {
                                    name.on_hover_text(r.name.as_str());
                                }
                                let pattern = ui.add_sized(
                                    [PATTERN_W, 20.0],
                                    egui::TextEdit::singleline(&mut r.pattern)
                                        .font(TextStyle::Monospace)
                                        .hint_text(t!("rules.pattern.hint")),
                                );
                                changed |= pattern.changed();
                                if !r.pattern.is_empty() {
                                    pattern.on_hover_text(
                                        RichText::new(r.pattern.as_str()).monospace(),
                                    );
                                }
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
                                            ui.selectable_value(&mut r.scope, s, scope_label(s));
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
                    // The export choice moves with its rule.
                    if let Some(p) = &mut self.rules_export_pick {
                        p.resize(self.cfg.rules.len(), true);
                        move_rule(p, i, d);
                    }
                    move_rule(&mut self.cfg.rules, i, d);
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
                                let res = crate::config::import_rules(&mut self.cfg.rules, rules);
                                self.rules_msg = Some(t!(
                                    "rules.import.done",
                                    added = res.added,
                                    updated = res.updated,
                                    skipped = res.skipped
                                ));
                                changed = true;
                            }
                            Err(e) => self.status_error = Some(t!("error.import", error = e)),
                        }
                    }
                    let picking = self.rules_export_pick.is_some();
                    if ui
                        .selectable_label(picking, format!("📤 {}", t!("rules.export")))
                        .on_hover_text(t!("rules.export.tip"))
                        .clicked()
                    {
                        self.rules_export_pick =
                            (!picking).then(|| vec![true; self.cfg.rules.len()]);
                    }
                    if ui.button(t!("rules.restore_defaults")).clicked() {
                        self.cfg.rules = Config::default_rules();
                        if self.rules_export_pick.is_some() {
                            self.rules_export_pick = Some(vec![true; self.cfg.rules.len()]);
                        }
                        changed = true;
                    }
                });
                self.rules_export_choice(ui);
                if let Some(m) = &self.rules_msg {
                    ui.label(m.as_str());
                }
            });
        if !open {
            self.rules_export_pick = None;
            self.rules_msg = None;
        }
        self.show_rules = open;
        self.rules_was_open = open;
        if changed {
            self.recompile_rules();
            self.cfg_dirty = true;
        }
    }

    /// The rules to export with a checkbox each (all ticked at first) and the export button (#51).
    fn rules_export_choice(&mut self, ui: &mut egui::Ui) {
        let Some(pick) = &mut self.rules_export_pick else {
            return;
        };
        pick.resize(self.cfg.rules.len(), true);
        ui.separator();
        ui.label(t!("rules.export.pick"));
        egui::ScrollArea::vertical()
            .id_salt("rules_export_pick")
            .max_height(PICK_H)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (i, (r, p)) in self.cfg.rules.iter().zip(pick.iter_mut()).enumerate() {
                    ui.push_id(("pick", i), |ui| {
                        let name = if r.name.is_empty() {
                            &r.pattern
                        } else {
                            &r.name
                        };
                        ui.checkbox(p, name.as_str())
                            .on_hover_text(RichText::new(r.pattern.as_str()).monospace());
                    });
                }
            });
        let count = pick.iter().filter(|p| **p).count();
        let mut save = false;
        let mut close = false;
        ui.horizontal(|ui| {
            if ui.button(t!("tabexport.columns.all")).clicked() {
                pick.iter_mut().for_each(|p| *p = true);
            }
            if ui.button(t!("tabexport.columns.none")).clicked() {
                pick.iter_mut().for_each(|p| *p = false);
            }
            save = ui
                .add_enabled(
                    count > 0,
                    egui::Button::new(format!("📤 {}", t!("rules.export.save", count = count))),
                )
                .clicked();
            close = ui.button(t!("progress.cancel")).clicked();
        });
        if save
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("JSON", &["json"])
                .set_file_name("rustrace-rules.json")
                .save_file()
        {
            let rules = picked_rules(&self.cfg.rules, pick);
            let json = serde_json::to_string_pretty(&rules).unwrap_or_default();
            match std::fs::write(&path, json) {
                Ok(()) => {
                    self.rules_msg = Some(t!(
                        "rules.export.done",
                        count = rules.len(),
                        path = path.display()
                    ));
                    close = true;
                }
                Err(e) => self.status_error = Some(t!("error.export", error = e)),
            }
        }
        if close {
            self.rules_export_pick = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_window_width_fits_the_screen() {
        // Large screen: the preferred width (the whole table is visible).
        assert_eq!(default_window_width(1920.0), WINDOW_W);
        // 1400 px application window (#48): still the full width.
        assert_eq!(default_window_width(1400.0), WINDOW_W);
        // Smaller window: a margin is kept, the table scrolls horizontally.
        assert_eq!(default_window_width(1024.0), 984.0);
        // Tiny window: never under the minimum width.
        assert_eq!(default_window_width(500.0), MIN_WINDOW_W);
    }

    #[test]
    fn exporting_ticked_rules_only() {
        let rules = Config::default_rules();
        assert!(rules.len() >= 3);
        let mut pick = vec![false; rules.len()];
        pick[0] = true;
        pick[2] = true;
        let json = serde_json::to_string_pretty(&picked_rules(&rules, &pick)).unwrap();
        // The file holds just the ticked rules and imports again.
        let back: Vec<HighlightRule> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, vec![rules[0].clone(), rules[2].clone()]);
        let mut empty = Vec::new();
        assert_eq!(crate::config::import_rules(&mut empty, back).added, 2);
        assert_eq!(empty[1].name, rules[2].name);
    }

    #[test]
    fn export_choice_moves_with_the_rule() {
        let mut pick = vec![true, false, true];
        move_rule(&mut pick, 1, -1);
        assert_eq!(pick, [false, true, true]);
        move_rule(&mut pick, 0, 1);
        assert_eq!(pick, [true, false, true]);
        move_rule(&mut pick, 1, 0);
        assert_eq!(pick, [true, true]);
    }
}
