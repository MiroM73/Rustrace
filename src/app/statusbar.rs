//! The status bar.

use crate::app::*;

impl App {
    pub(in crate::app) fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(path) = &self.cur_path {
                ui.label(path.display().to_string());
                ui.separator();
                if !self.encoding_name.is_empty() {
                    ui.label(self.encoding_name);
                    ui.separator();
                }
                let warn = if self.competing_defs().is_empty() { "" } else { " ⚠" };
                ui.label(format!("📐 {}{warn}", self.format_name()))
                    .on_hover_text(self.format_tip());
                ui.separator();
                if self.file_size > 0 {
                    ui.label(format_size(self.file_size));
                    ui.separator();
                }
                if self.dropped > 0 {
                    ui.colored_label(ORANGE, format!("✂ {}", t!("status.dropped", count = self.dropped)))
                        .on_hover_text(t!("status.dropped.tip", limit = self.cfg.max_log_mb));
                    ui.separator();
                }
                if self.files.len() > 1 {
                    ui.label(format!("📚 {}", t!("status.parts", count = self.files.len())))
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
            if let Some(r) = &self.update_available {
                ui.separator();
                let version = r.version.clone();
                if ui
                    .button(RichText::new(format!("⬆ {version}")).color(Color32::from_rgb(60, 170, 80)))
                    .on_hover_text(t!("update.available", version = version))
                    .clicked()
                {
                    self.show_update = true;
                    if matches!(self.update, UpdateState::Idle | UpdateState::Failed(_)) {
                        self.update = UpdateState::Checked(r.clone());
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
