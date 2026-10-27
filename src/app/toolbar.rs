//! The toolbar and the time range row.

use crate::app::*;

impl App {
    pub(in crate::app) fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(format!("📂 {}", t!("toolbar.open")))
                .on_hover_text(kb("Ctrl+O"))
                .clicked()
            {
                self.open_dialog();
            }
            if ui
                .add_enabled(
                    self.cur_path.is_some(),
                    egui::Button::new(format!("🔄 {}", t!("toolbar.refresh"))),
                )
                .on_hover_text(t!("toolbar.refresh.tip"))
                .clicked()
            {
                self.refresh();
            }
            ui.separator();

            let (label, tip) = if self.cfg.auto_refresh {
                (
                    format!("⏸ {}", t!("toolbar.pause")),
                    t!("toolbar.pause.tip"),
                )
            } else {
                (
                    format!("▶ {}", t!("toolbar.resume")),
                    t!("toolbar.resume.tip"),
                )
            };
            let btn = egui::Button::new(label).selected(!self.cfg.auto_refresh);
            if ui.add(btn).on_hover_text(tip).clicked() {
                self.cfg.auto_refresh = !self.cfg.auto_refresh;
                self.cfg_dirty = true;
            }

            ui.label(t!("toolbar.interval"));
            let unit = self.cfg.refresh_unit;
            let speed = match unit {
                IntervalUnit::Ms => 10.0,
                IntervalUnit::Sec => 0.1,
                IntervalUnit::Min => 0.05,
            };
            if ui
                .add(
                    egui::DragValue::new(&mut self.cfg.refresh_value)
                        .range(unit.range())
                        .speed(speed)
                        .max_decimals(2),
                )
                .changed()
            {
                self.cfg_dirty = true;
            }
            let old_unit = self.cfg.refresh_unit;
            egui::ComboBox::from_id_salt("interval_unit")
                .selected_text(self.cfg.refresh_unit.label())
                .width(55.0)
                .show_ui(ui, |ui| {
                    for u in IntervalUnit::ALL {
                        ui.selectable_value(&mut self.cfg.refresh_unit, u, u.label());
                    }
                });
            if old_unit != self.cfg.refresh_unit {
                // Convert so the interval stays the same (e.g. 1500 ms → 1.5 s).
                let ms = old_unit.to_ms(self.cfg.refresh_value);
                let new = ms / self.cfg.refresh_unit.to_ms(1.0);
                let r = self.cfg.refresh_unit.range();
                self.cfg.refresh_value =
                    ((new * 100.0).round() / 100.0).clamp(*r.start(), *r.end());
                self.cfg_dirty = true;
            }
            ui.menu_button("⏱", |ui| {
                ui.label(t!("toolbar.presets"));
                for (v, u) in [
                    (100.0, IntervalUnit::Ms),
                    (250.0, IntervalUnit::Ms),
                    (500.0, IntervalUnit::Ms),
                    (1.0, IntervalUnit::Sec),
                    (2.0, IntervalUnit::Sec),
                    (5.0, IntervalUnit::Sec),
                    (10.0, IntervalUnit::Sec),
                    (30.0, IntervalUnit::Sec),
                    (1.0, IntervalUnit::Min),
                    (5.0, IntervalUnit::Min),
                ] {
                    if ui.button(format!("{v} {}", u.label())).clicked() {
                        self.cfg.refresh_value = v;
                        self.cfg.refresh_unit = u;
                        self.cfg.auto_refresh = true;
                        self.cfg_dirty = true;
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text(t!("toolbar.presets"));

            if ui
                .checkbox(&mut self.cfg.auto_scroll, t!("toolbar.follow"))
                .on_hover_text(t!("toolbar.follow.tip"))
                .changed()
            {
                self.cfg_dirty = true;
                if self.cfg.auto_scroll && !self.filtered.is_empty() {
                    self.scroll_to = Some(self.filtered.len() - 1);
                }
            }

            ui.separator();
            let mut sev_changed = false;
            sev_changed |= ui
                .toggle_value(
                    &mut self.show_sev[0],
                    format!("ℹ {}", t!("severity.info.plural")),
                )
                .changed();
            sev_changed |= ui
                .toggle_value(
                    &mut self.show_sev[1],
                    format!("⚠ {}", t!("severity.warning.plural")),
                )
                .changed();
            sev_changed |= ui
                .toggle_value(
                    &mut self.show_sev[2],
                    format!("⛔ {}", t!("severity.error.plural")),
                )
                .changed();
            if sev_changed {
                self.refilter_from(0);
            }
        });

        // Search row
        ui.horizontal_wrapped(|ui| {
            ui.label("🔍");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.search_text)
                    .hint_text(format!("{}  ({})", t!("search.hint"), kb("Ctrl+F")))
                    .desired_width(340.0)
                    .font(TextStyle::Monospace),
            );
            if self.focus_search {
                resp.request_focus();
                self.focus_search = false;
            }
            let mut changed = resp.changed();
            // Search as you type (#4): the first match is selected right away.
            let typed = changed;
            if typed {
                self.search_confirmed = false;
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let back = ui.input(|i| i.modifiers.shift);
                self.find(!back);
                resp.request_focus();
            } else if resp.lost_focus() {
                self.remember_search();
            }
            // Search history: earlier search texts, newest first.
            let mut picked = None;
            ui.menu_button("🕘", |ui| {
                if self.cfg.search_history.is_empty() {
                    ui.weak(t!("search.history.none"));
                }
                for text in &self.cfg.search_history {
                    let btn = egui::Button::new(RichText::new(text).monospace()).truncate();
                    if ui.add(btn).on_hover_text(text).clicked() {
                        picked = Some(text.clone());
                        ui.close();
                    }
                }
                if !self.cfg.search_history.is_empty() {
                    ui.separator();
                    if ui
                        .button(format!("🗑 {}", t!("search.history.clear")))
                        .clicked()
                    {
                        self.cfg.search_history.clear();
                        self.cfg_dirty = true;
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text(t!("search.history.tip"));
            if let Some(text) = picked {
                self.search_text = text;
                self.search_confirmed = true;
                self.remember_search();
                changed = true;
            }
            // Match count (or the regex error) and the clear button sit right next to the
            // search field, so the result of the search is visible where the user types.
            if let Some(err) = &self.search_err {
                ui.colored_label(RED, format!("⚠ {}", t!("search.invalid_regex")))
                    .on_hover_text(err);
            } else if self.search_re.is_some() {
                ui.label(t!("search.matches", count = self.is_match.count()));
            } else if !self.search_text.is_empty() {
                ui.weak(t!("search.short", count = INSTANT_SEARCH_MIN_CHARS));
            }
            if !self.search_text.is_empty()
                && ui
                    .small_button("✖")
                    .on_hover_text(t!("search.clear.tip"))
                    .clicked()
            {
                self.search_text.clear();
                changed = true;
            }
            changed |= ui
                .checkbox(&mut self.search_regex, "Regex")
                .on_hover_text(t!("search.regex.tip"))
                .changed();
            changed |= ui
                .checkbox(&mut self.search_case, "Aa")
                .on_hover_text(t!("search.case.tip"))
                .changed();
            changed |= ui
                .checkbox(&mut self.filter_mode, t!("search.filter"))
                .on_hover_text(t!("search.filter.tip"))
                .changed();
            if changed {
                self.recompile_search();
                if typed && self.search_re.is_some() {
                    self.select_first_match();
                }
            }
            let active = self.col_filters.len();
            let label = if active > 0 {
                t!("toolbar.column_filters.active", count = active)
            } else {
                t!("toolbar.column_filters")
            };
            if ui
                .toggle_value(&mut self.cfg.show_column_filters, label)
                .on_hover_text(t!("toolbar.column_filters.tip"))
                .changed()
            {
                self.cfg_dirty = true;
            }
            if ui
                .toggle_value(
                    &mut self.bookmarks_only,
                    format!("🔖 {}", t!("toolbar.bookmarks_only")),
                )
                .on_hover_text(t!("toolbar.bookmarks_only.tip"))
                .changed()
            {
                self.refilter_from(0);
            }
            let label = if self.time_range_active() {
                t!("toolbar.time_range.active")
            } else {
                t!("toolbar.time_range")
            };
            if ui
                .toggle_value(&mut self.cfg.show_time_range, format!("🕑 {label}"))
                .on_hover_text(t!("toolbar.time_range.tip"))
                .changed()
            {
                self.cfg_dirty = true;
            }
            if ui
                .button("⬆")
                .on_hover_text(t!("search.prev.tip"))
                .clicked()
            {
                self.find(false);
            }
            if ui
                .button("⬇")
                .on_hover_text(t!("search.next.tip"))
                .clicked()
            {
                self.find(true);
            }
            ui.separator();
            let (icon, name) = match self.cfg.theme.as_str() {
                "light" => ("☀", t!("menu.view.theme.light")),
                "dark" => ("🌙", t!("menu.view.theme.dark")),
                _ => ("🖥", t!("menu.view.theme.system")),
            };
            if ui
                .button(icon)
                .on_hover_text(t!("toolbar.theme.tip", theme = name))
                .clicked()
            {
                let ctx = ui.ctx().clone();
                self.cycle_theme(&ctx);
            }
            if ui
                .button(format!("🎨 {}", t!("toolbar.highlighting")))
                .on_hover_text(kb("Ctrl+H"))
                .clicked()
            {
                self.show_rules = true;
            }
        });
        if self.cfg.show_time_range {
            self.time_range_row(ui);
        }
        ui.add_space(2.0);
    }

    /// Time range filter: from / to fields with presets.
    pub(in crate::app) fn time_range_row(&mut self, ui: &mut egui::Ui) {
        let latest = self.latest_ts();
        let example = self.input_dt(latest.as_deref().unwrap_or("2026-10-23 08:00:00.000"));
        let selected = self
            .selected
            .and_then(|i| self.entries.get(i))
            .and_then(|e| e.timestamp())
            .map(|ms| self.input_dt(&ms_to_iso(ms)));
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("🕑 {}", t!("time.from")));
            fn field<'a>(text: &'a mut String, hint: &str) -> egui::TextEdit<'a> {
                egui::TextEdit::singleline(text)
                    .hint_text(hint)
                    .desired_width(170.0)
                    .font(TextStyle::Monospace)
            }
            changed |= ui
                .add(field(&mut self.time_from, &example))
                .on_hover_text(t!("time.tip"))
                .changed();
            if self.time_invalid.0 {
                ui.colored_label(RED, "⚠")
                    .on_hover_text(t!("time.invalid", example = example));
            }
            ui.label(t!("time.to"));
            changed |= ui
                .add(field(&mut self.time_to, &example))
                .on_hover_text(t!("time.tip"))
                .changed();
            if self.time_invalid.1 {
                ui.colored_label(RED, "⚠")
                    .on_hover_text(t!("time.invalid", example = example));
            }
            ui.separator();
            if ui
                .add_enabled(
                    selected.is_some(),
                    egui::Button::new(t!("time.from_selected")),
                )
                .clicked()
            {
                self.time_from = selected.clone().unwrap_or_default();
                changed = true;
            }
            if ui
                .add_enabled(
                    selected.is_some(),
                    egui::Button::new(t!("time.to_selected")),
                )
                .clicked()
            {
                self.time_to = selected.clone().unwrap_or_default();
                changed = true;
            }
            for (hours, key) in [(1, "export.since.last_hour"), (24, "export.since.last_day")] {
                if ui
                    .add_enabled(latest.is_some(), egui::Button::new(t!(key)))
                    .on_hover_text(t!("export.since.presets.tip"))
                    .clicked()
                {
                    let from = latest
                        .as_deref()
                        .and_then(|l| export::hours_before(l, hours));
                    self.time_from = from.map(|f| self.input_dt(&f)).unwrap_or_default();
                    self.time_to.clear();
                    changed = true;
                }
            }
            if ui
                .add_enabled(
                    !self.time_from.is_empty() || !self.time_to.is_empty(),
                    egui::Button::new(format!("✖ {}", t!("time.clear"))),
                )
                .clicked()
            {
                self.time_from.clear();
                self.time_to.clear();
                changed = true;
            }
            if self.cur_path.is_some() && latest.is_none() && !self.entries.is_empty() {
                ui.colored_label(ORANGE, t!("time.no_time"));
            }
        });
        if changed {
            self.apply_time_range();
        }
    }
}
