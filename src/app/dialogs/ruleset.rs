//! The rule set bar of the Highlight rules window (#70): choose, switch on, bind to log
//! definitions, create / rename / duplicate / delete, add a built-in set, import a file.

use crate::app::*;
use crate::rulesets::{self, RulesFile};

impl App {
    /// The bar above the table of rules. Returns whether the settings changed.
    pub(in crate::app) fn rule_set_bar(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        self.rules_set = self.rules_set.min(self.cfg.rule_sets.len() - 1);
        let mut select = None;
        let label = |s: &RuleSet| {
            if s.enabled {
                s.name.clone()
            } else {
                format!("{} ({})", s.name, t!("rules.set.off"))
            }
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(t!("rules.set"));
            egui::ComboBox::from_id_salt("rule_set")
                .selected_text(label(&self.cfg.rule_sets[self.rules_set]))
                .show_ui(ui, |ui| {
                    for (i, s) in self.cfg.rule_sets.iter().enumerate() {
                        if ui.selectable_label(i == self.rules_set, label(s)).clicked() {
                            select = Some(i);
                        }
                    }
                });
            let si = self.rules_set;
            changed |= ui
                .checkbox(&mut self.cfg.rule_sets[si].enabled, t!("rules.set.active"))
                .on_hover_text(t!("rules.set.active.tip"))
                .changed();
            changed |= self.binding_menu(ui);
            if ui
                .button(format!("➕ {}", t!("rules.set.new")))
                .on_hover_text(t!("rules.set.new.tip"))
                .clicked()
            {
                let name = self.unique_set_name(&t!("rules.set.new_name"));
                self.cfg.rule_sets.push(RuleSet::new(name, Vec::new()));
                select = Some(self.cfg.rule_sets.len() - 1);
                changed = true;
            }
            if ui.button(format!("✏ {}", t!("rules.set.rename"))).clicked() {
                self.rules_rename = match self.rules_rename {
                    Some(_) => None,
                    None => Some(self.cfg.rule_sets[si].name.clone()),
                };
            }
            let n = self.cfg.rule_sets.len();
            for (text, to, tip) in [
                ("⬅", si.wrapping_sub(1), "rules.set.up.tip"),
                ("➡", si + 1, "rules.set.down.tip"),
            ] {
                if ui
                    .add_enabled(to < n, egui::Button::new(text))
                    .on_hover_text(t!(tip))
                    .clicked()
                {
                    self.cfg.rule_sets.swap(si, to);
                    select = Some(to);
                    changed = true;
                }
            }
            if ui
                .button(format!("📄 {}", t!("rules.set.duplicate")))
                .clicked()
            {
                let mut copy = self.cfg.rule_sets[si].clone();
                copy.name = self.unique_set_name(&t!("rules.set.copy_name", name = copy.name));
                self.cfg.rule_sets.insert(si + 1, copy);
                select = Some(si + 1);
                changed = true;
            }
            if ui
                .add_enabled(
                    self.cfg.rule_sets.len() > 1,
                    egui::Button::new(format!("🗑 {}", t!("rules.set.delete"))),
                )
                .on_hover_text(t!("rules.set.delete.tip"))
                .clicked()
            {
                self.cfg.rule_sets.remove(si);
                select = Some(si.saturating_sub(1));
                changed = true;
            }
            ui.menu_button(format!("📚 {}", t!("rules.set.builtin")), |ui| {
                for f in rulesets::builtin_sets() {
                    if ui.button(&f.name).clicked() {
                        let (i, res) = rulesets::import_set(&mut self.cfg.rule_sets, f);
                        self.rules_msg = Some(import_message(&res));
                        select = Some(i);
                        changed = true;
                        ui.close();
                    }
                }
            });
        });
        if let Some(i) = select {
            self.rules_set = i;
            self.rules_rename = None;
            self.rules_export_pick = None;
        }
        changed | self.rename_row(ui)
    }

    /// The name field of a set being renamed; Enter applies it.
    fn rename_row(&mut self, ui: &mut egui::Ui) -> bool {
        let si = self.rules_set;
        let Some(text) = self.rules_rename.clone() else {
            return false;
        };
        let mut text = text;
        let taken = |n: &str| {
            self.cfg
                .rule_sets
                .iter()
                .enumerate()
                .any(|(i, s)| i != si && s.name == n)
        };
        let mut apply = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut text).desired_width(240.0));
            let n = text.trim();
            let ok = !n.is_empty() && !taken(n);
            if !ok && !n.is_empty() {
                ui.colored_label(RED, t!("rules.set.name_taken"));
            }
            apply = ui.add_enabled(ok, egui::Button::new("✔")).clicked()
                || (ok && edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            cancel = ui.button("✖").clicked();
        });
        if apply {
            self.cfg.rule_sets[si].name = text.trim().to_owned();
            self.rules_rename = None;
            return true;
        }
        self.rules_rename = (!cancel).then_some(text);
        false
    }

    /// The log definitions the set is bound to (none = every log).
    fn binding_menu(&mut self, ui: &mut egui::Ui) -> bool {
        let si = self.rules_set;
        let count = self.cfg.rule_sets[si].logdefs.len();
        let label = if count == 0 {
            t!("rules.set.bind.all")
        } else {
            t!("rules.set.bind.n", count = count)
        };
        let mut changed = false;
        // Known definitions first, then ids of the set that are not known (a definition of
        // another computer): they stay until unticked.
        let mut ids: Vec<(String, String)> = Vec::new();
        for d in self.defs.iter() {
            if !ids.iter().any(|(id, _)| *id == d.id) {
                ids.push((d.id.clone(), format!("{} ({})", d.name, d.id)));
            }
        }
        for id in &self.cfg.rule_sets[si].logdefs {
            if !ids.iter().any(|(i, _)| i.eq_ignore_ascii_case(id)) {
                ids.push((id.clone(), id.clone()));
            }
        }
        ui.menu_button(format!("🔗 {label}"), |ui| {
            ui.label(t!("rules.set.bind.help"));
            let defs = &mut self.cfg.rule_sets[si].logdefs;
            if ui
                .add_enabled(
                    !defs.is_empty(),
                    egui::Button::new(t!("rules.set.bind.clear")),
                )
                .clicked()
            {
                defs.clear();
                changed = true;
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    for (id, text) in &ids {
                        let pos = defs.iter().position(|d| d.eq_ignore_ascii_case(id));
                        let mut on = pos.is_some();
                        if ui.checkbox(&mut on, text.as_str()).changed() {
                            match pos {
                                Some(p) => {
                                    defs.remove(p);
                                }
                                None => defs.push(id.clone()),
                            }
                            changed = true;
                        }
                    }
                });
        });
        changed
    }

    /// `base`, or `base 2`, `base 3` … when a set has that name.
    fn unique_set_name(&self, base: &str) -> String {
        let taken = |n: &str| self.cfg.rule_sets.iter().any(|s| s.name == n);
        if !taken(base) {
            return base.to_owned();
        }
        (2..)
            .map(|k| format!("{base} {k}"))
            .find(|n| !taken(n))
            .unwrap_or_default()
    }

    /// *Import…*: a set file becomes a set (or is merged into the set of the same name), a bare
    /// list of rules goes into the set shown. Returns whether the settings changed.
    pub(in crate::app) fn import_rules_file(&mut self) -> bool {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Rules", &["rules", "json"])
            .pick_file()
        else {
            return false;
        };
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| rulesets::parse_rules_file(&s));
        match parsed {
            Ok(RulesFile::Set(f)) => {
                let (i, res) = rulesets::import_set(&mut self.cfg.rule_sets, f);
                self.rules_msg = Some(import_message(&res));
                self.rules_set = i;
                self.rules_export_pick = None;
                true
            }
            Ok(RulesFile::Rules(rules)) => {
                let si = self.rules_set;
                let res = crate::config::import_rules(&mut self.cfg.rule_sets[si].rules, rules);
                self.rules_msg = Some(import_message(&res));
                true
            }
            Err(e) => {
                self.status_error = Some(t!("error.import", error = e));
                false
            }
        }
    }
}

fn import_message(res: &crate::config::RuleImport) -> String {
    t!(
        "rules.import.done",
        added = res.added,
        updated = res.updated,
        skipped = res.skipped
    )
}
