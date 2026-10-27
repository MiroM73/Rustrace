//! The status bar.

use crate::app::*;

impl App {
    pub(in crate::app) fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(path) = &self.cur_path {
                if self.is_merged() {
                    ui.label(format!(
                        "📚 {}",
                        t!("status.merged", count = self.paths.len())
                    ))
                    .on_hover_ui(|ui| self.merged_tip(ui));
                } else {
                    ui.label(path.display().to_string());
                }
                ui.separator();
                if !self.encoding_name.is_empty() {
                    ui.label(self.encoding_name);
                    ui.separator();
                }
                let warn = if self.competing_defs().is_empty() {
                    ""
                } else {
                    " ⚠"
                };
                ui.label(format!("📐 {}{warn}", self.format_name()))
                    .on_hover_text(self.format_tip());
                ui.separator();
                if self.file_size > 0 {
                    ui.label(format_size(self.file_size));
                    ui.separator();
                }
                if self.dropped > 0 {
                    ui.colored_label(
                        ORANGE,
                        format!("✂ {}", t!("status.dropped", count = self.dropped)),
                    )
                    .on_hover_text(t!("status.dropped.tip", limit = self.cfg.max_log_mb));
                    ui.separator();
                }
                if self.files.len() > 1 && !self.is_merged() {
                    ui.label(format!(
                        "📚 {}",
                        t!("status.parts", count = self.files.len())
                    ))
                    .on_hover_text(self.file_labels.join("\n"));
                    ui.separator();
                }
                if self.entries.is_mapped() {
                    ui.label("💾").on_hover_text(t!(
                        "status.temp_file",
                        size = format_size(self.entries.text_len() as u64)
                    ));
                    ui.separator();
                }
                // Background re-filter (#16): the counts below are still the previous result.
                if let Some(running) = self.filter_running_for() {
                    if running >= FILTER_HINT_DELAY {
                        ui.spinner();
                        ui.label(t!("status.searching"));
                        ui.separator();
                    } else {
                        ui.ctx().request_repaint_after(FILTER_HINT_DELAY - running);
                    }
                }
                if self.filtered.len() == self.entries.len() {
                    ui.label(t!("status.entries", count = self.entries.len()));
                } else {
                    ui.label(t!(
                        "status.shown",
                        shown = self.filtered.len(),
                        total = self.entries.len()
                    ));
                }
                ui.separator();
                // Position of the selected row among the displayed rows (#3).
                if let Some(sel) = self.selected
                    && let Ok(pos) = self.filtered.binary_search(&sel)
                {
                    let total = self.filtered.len();
                    let line = self.entries.get(sel).map_or(0, |e| e.line_no);
                    ui.label(format!(
                        "📍 {}",
                        t!(
                            "status.position",
                            row = pos + 1,
                            total = total,
                            percent = position_percent(pos + 1, total)
                        )
                    ))
                    .on_hover_text(t!("status.position.tip", line = line));
                    ui.separator();
                }
                if let Some(s) = self.selection_summary() {
                    ui.label(format!("☑ {s}"));
                    ui.separator();
                }
                if !self.bookmark_rows.is_empty() {
                    if ui
                        .button(format!("🔖 {}", self.bookmark_rows.len()))
                        .on_hover_text(t!("status.bookmarks.tip", count = self.bookmark_rows.len()))
                        .clicked()
                    {
                        self.show_bookmarks = true;
                    }
                    ui.separator();
                }
                if self.cfg.auto_refresh {
                    ui.label(format!(
                        "🔄 {}",
                        t!(
                            "status.refresh_every",
                            value = trim_num(self.cfg.refresh_value),
                            unit = self.cfg.refresh_unit.label()
                        )
                    ));
                } else {
                    ui.colored_label(ORANGE, format!("⏸ {}", t!("status.paused")));
                }
                if let Some(time) = &self.last_refresh_wall {
                    ui.separator();
                    ui.label(t!("status.last_change", time = time.format("%H:%M:%S")));
                }
            } else {
                ui.label(t!("status.no_file"));
            }
            if let Some(u) = &self.update_available {
                ui.separator();
                let version = u.latest.version.clone();
                if ui
                    .button(
                        RichText::new(format!("⬆ {version}")).color(Color32::from_rgb(60, 170, 80)),
                    )
                    .on_hover_text(t!("update.available", version = version))
                    .clicked()
                {
                    self.show_update = true;
                    if matches!(self.update, UpdateState::Idle | UpdateState::Failed(_)) {
                        self.update = UpdateState::Checked(u.clone());
                    }
                }
            }
            // During the first load the progress is in the middle of the window; here only for later refreshes.
            if self.loading.is_some() && !self.entries.is_empty() {
                self.progress_ui(ui, false);
            }
            if let Some(e) = &self.status_error {
                ui.separator();
                ui.colored_label(RED, e);
            }
            if let Some((msg, time)) = &self.status_info
                && time.elapsed() < Duration::from_secs(3)
            {
                ui.separator();
                ui.colored_label(ORANGE, msg);
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        });
    }
}

impl App {
    /// Tooltip of a merged view: every log with its format, its files, their rows and encodings.
    fn merged_tip(&self, ui: &mut egui::Ui) {
        let mut rows = vec![0usize; self.files.len()];
        for i in 0..self.entries.len() {
            if let Some(r) = rows.get_mut(self.entries.file(i) as usize) {
                *r += 1;
            }
        }
        for (s, p) in self.paths.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("■").color(self.log_color(s)));
                ui.strong(p.display().to_string());
            });
            let format = match self.source_defs.get(s) {
                Some(Some(d)) => format!("{} – {}", d.name, d.id),
                _ => t!("format.builtin"),
            };
            ui.label(format!("    📐 {format}"));
            for (f, file) in self.files.iter().enumerate() {
                if self.file_sources.get(f) == Some(&s) {
                    ui.label(format!(
                        "    {} – {}, {}",
                        self.file_labels.get(f).map_or("", |l| l.as_str()),
                        t!("status.rows", count = rows[f]),
                        file.encoding.name()
                    ));
                }
            }
        }
        ui.separator();
        ui.weak(t!("status.merged.tip"));
    }

    /// Color of a log in a merged view (File column, menu, status bar).
    pub(in crate::app) fn log_color(&self, log: usize) -> Color32 {
        const LIGHT: [(u8, u8, u8); 8] = [
            (0, 110, 200),
            (200, 90, 0),
            (0, 140, 70),
            (150, 60, 180),
            (190, 40, 90),
            (0, 130, 140),
            (130, 105, 0),
            (100, 100, 100),
        ];
        const DARK: [(u8, u8, u8); 8] = [
            (90, 170, 255),
            (255, 160, 70),
            (80, 210, 120),
            (200, 140, 255),
            (255, 110, 160),
            (70, 210, 220),
            (225, 200, 80),
            (180, 180, 180),
        ];
        let (r, g, b) = if self.dark { DARK } else { LIGHT }[log % 8];
        Color32::from_rgb(r, g, b)
    }
}

pub(in crate::app) fn format_size(b: u64) -> String {
    let b = b as f64;
    if b >= 1024.0 * 1024.0 {
        format!("{:.1} MB", b / 1024.0 / 1024.0)
    } else if b >= 1024.0 {
        format!("{:.1} kB", b / 1024.0)
    } else {
        format!("{b} B")
    }
}

pub(in crate::app) fn trim_num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Percentage of `row` (1-based) out of `total` rows, rounded down; the last row is 100 %.
pub(in crate::app) fn position_percent(row: usize, total: usize) -> u64 {
    if total == 0 {
        return 0;
    }
    row.min(total) as u64 * 100 / total as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_percent_of_rows() {
        assert_eq!(position_percent(25, 100), 25);
        assert_eq!(position_percent(1, 3), 33);
        assert_eq!(position_percent(3, 3), 100);
        assert_eq!(position_percent(1, 1), 100);
        assert_eq!(position_percent(0, 0), 0);
        assert_eq!(position_percent(1, 1_800_000), 0);
    }
}
