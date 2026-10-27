//! The Δ time column (#23): time since the previous shown row, the previous row of the log or
//! a reference row, computed from the stored timestamps only for the rows that are drawn.

use crate::app::*;
use crate::timefmt::format_delta;

/// How many rows back a dated row is looked for (rows without a timestamp in between).
const MAX_LOOK_BACK: usize = 1000;

impl App {
    /// Δ of an entry in milliseconds for the chosen mode; `None` for rows without a timestamp
    /// or without an earlier dated row.
    pub(in crate::app) fn delta_ms(&self, idx: usize, mode: DeltaMode) -> Option<i64> {
        let ts = self.entries.get(idx)?.timestamp()?;
        let prev = match mode {
            DeltaMode::Reference => return self.time_ref.map(|(r, _, _)| ts - r),
            // The rows shown above it (a hidden row: the shown rows before it).
            DeltaMode::Visible => {
                let p = self.filtered.partition_point(|&f| f < idx);
                self.filtered[..p]
                    .iter()
                    .rev()
                    .take(MAX_LOOK_BACK)
                    .find_map(|&i| self.entries.timestamp(i))
            }
            DeltaMode::Log => (idx.saturating_sub(MAX_LOOK_BACK)..idx)
                .rev()
                .find_map(|i| self.entries.timestamp(i)),
        }?;
        Some(ts - prev)
    }

    /// Text of the Δ cell and whether it is a long gap (colored).
    pub(in crate::app) fn delta_cell(&self, idx: usize) -> (String, bool) {
        let mode = self.cfg.delta_mode;
        match self.delta_ms(idx, mode) {
            Some(d) => {
                let gap = self.cfg.delta_highlight
                    && mode != DeltaMode::Reference
                    && d as f64 >= self.cfg.delta_threshold_s * 1000.0;
                (format_delta(d), gap)
            }
            None => (String::new(), false),
        }
    }

    /// Text of any column of an entry, the Δ column included.
    pub(in crate::app) fn cell(&self, idx: usize, kind: ColumnKind) -> String {
        match kind {
            ColumnKind::Delta => self.delta_cell(idx).0,
            _ => self.cell_text(&self.entries.at(idx), kind),
        }
    }

    /// Right click → "Set as time reference".
    pub(in crate::app) fn set_time_reference(&mut self, idx: usize) {
        let Some(e) = self.entries.get(idx) else {
            return;
        };
        match e.timestamp() {
            Some(ts) => {
                self.time_ref = Some((ts, e.file, e.line_no));
                self.cfg.delta_column = true;
                self.cfg.delta_mode = DeltaMode::Reference;
                self.cfg_dirty = true;
            }
            None => self.info(t!("delta.no_time")),
        }
    }

    /// Settings of the Δ column (View menu and the column headers' context menu).
    pub(in crate::app) fn delta_menu(&mut self, ui: &mut egui::Ui) {
        let mut changed = ui
            .checkbox(&mut self.cfg.delta_column, t!("delta.column"))
            .on_hover_text(t!("delta.column.tip"))
            .changed();
        ui.add_enabled_ui(self.cfg.delta_column, |ui| {
            for (mode, key) in [
                (DeltaMode::Visible, "delta.mode.visible"),
                (DeltaMode::Log, "delta.mode.log"),
                (DeltaMode::Reference, "delta.mode.reference"),
            ] {
                changed |= ui
                    .radio_value(&mut self.cfg.delta_mode, mode, t!(key))
                    .changed();
            }
            if let Some((_, file, line)) = self.time_ref {
                let at = if self.files.len() > 1 {
                    format!("{}:{line}", self.file_label(file))
                } else {
                    line.to_string()
                };
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.weak(t!("delta.reference.line", line = at));
                    if ui.small_button("✖").clicked() {
                        self.time_ref = None;
                    }
                });
            } else if self.cfg.delta_mode == DeltaMode::Reference {
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.weak(t!("delta.reference.none"));
                });
            }
            ui.horizontal(|ui| {
                changed |= ui
                    .checkbox(&mut self.cfg.delta_highlight, t!("delta.highlight"))
                    .on_hover_text(t!("delta.highlight.tip"))
                    .changed();
                changed |= ui
                    .add_enabled(
                        self.cfg.delta_highlight,
                        egui::DragValue::new(&mut self.cfg.delta_threshold_s)
                            .range(0.001..=86_400.0)
                            .speed(0.5)
                            .max_decimals(3)
                            .suffix(" s"),
                    )
                    .changed();
            });
        });
        if changed {
            self.cfg_dirty = true;
        }
    }
}
