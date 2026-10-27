//! Overview strip next to the table's vertical scroll bar (#24): errors, warnings, search
//! matches and bookmarks of the whole (filtered) log, the visible part, click to scroll there.

use crate::app::*;

/// Width of the strip in points.
pub(in crate::app) const STRIP_WIDTH: f32 = 10.0;

/// Marks, the higher one wins in a pixel.
const MARK_WARNING: u8 = 1;
const MARK_ERROR: u8 = 2;
const MARK_SEARCH: u8 = 3;
const MARK_BOOKMARK: u8 = 4;

impl App {
    /// The strip is drawn (turned on and there are rows).
    pub(in crate::app) fn strip_shown(&self) -> bool {
        self.cfg.overview.enabled && !self.filtered.is_empty()
    }

    /// The marks must be computed again from displayed row `pos` on.
    pub(in crate::app) fn overview_changed_from(&mut self, pos: usize) {
        self.overview_dirty = self.overview_dirty.min(pos);
    }

    /// Mark of one displayed row.
    fn row_mark(&self, pos: usize) -> u8 {
        let o = &self.cfg.overview;
        let idx = self.filtered[pos];
        if o.bookmarks && !self.bookmark_rows.is_empty() && self.is_bookmarked(idx) {
            MARK_BOOKMARK
        } else if o.search && self.is_match.get(idx) {
            MARK_SEARCH
        } else {
            match self.entries.severity(idx) {
                Severity::Error if o.errors => MARK_ERROR,
                Severity::Warning if o.warnings => MARK_WARNING,
                _ => 0,
            }
        }
    }

    /// Brings the marks up to date (only the changed end after appends).
    fn update_overview(&mut self) {
        let len = self.filtered.len();
        if self.overview_dirty == usize::MAX && self.overview.len() == len {
            return;
        }
        let from = self.overview_dirty.min(self.overview.len());
        let mut ov = std::mem::take(&mut self.overview);
        ov.update(from, len, |pos| self.row_mark(pos));
        self.overview = ov;
        self.overview_dirty = usize::MAX;
    }

    /// Draws the strip into `rect`; `visible` = displayed rows in view (first, last).
    pub(in crate::app) fn overview_strip(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        visible: Option<(usize, usize)>,
    ) {
        self.update_overview();
        let n = self.filtered.len();
        let height = rect.height().max(1.0);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
        // Every pixel row of the screen gets the highest mark of its rows.
        let ppp = ui.ctx().pixels_per_point();
        let px = (height * ppp).round().max(1.0) as usize;
        let p = self.cfg.palette(self.dark);
        let color = |m: u8| match m {
            MARK_BOOKMARK => c32(p.bookmark.1),
            MARK_SEARCH => c32(p.search.1),
            MARK_ERROR => c32(p.error.1),
            _ => c32(p.warning.1),
        };
        for (y, count, mark) in self.overview.runs(px) {
            let top = rect.top() + y as f32 / ppp;
            // At least 2 points, so a single row stays visible.
            let h = (count as f32 / ppp).max(2.0);
            let r = egui::Rect::from_min_size(
                egui::pos2(rect.left() + 1.0, top),
                egui::vec2(rect.width() - 2.0, h),
            );
            painter.rect_filled(r, 0.0, color(mark));
        }
        // The part of the log in view.
        if let Some((a, b)) = visible {
            let y0 = rect.top() + a as f32 / n as f32 * height;
            let y1 = rect.top() + (b + 1) as f32 / n as f32 * height;
            let r = egui::Rect::from_min_max(
                egui::pos2(rect.left(), y0),
                egui::pos2(rect.right(), y1.max(y0 + 3.0)),
            );
            let stroke = ui.visuals().widgets.active.fg_stroke.color;
            painter.rect_filled(r, 1.0, stroke.gamma_multiply(0.18));
            painter.rect_stroke(
                r,
                1.0,
                egui::Stroke::new(1.0, stroke.gamma_multiply(0.6)),
                egui::StrokeKind::Inside,
            );
        }
        // Click / drag scrolls there; the tooltip shows the row under the pointer.
        let resp = ui.interact(
            rect,
            ui.unique_id().with("overview_strip"),
            Sense::click_and_drag(),
        );
        let row_at = |y: f32| (((y - rect.top()) / height * n as f32) as usize).min(n - 1);
        if let Some(pos) = resp.interact_pointer_pos()
            && (resp.clicked() || resp.dragged())
        {
            self.scroll_to = Some(row_at(pos.y));
            self.cfg.auto_scroll = false;
        }
        if let Some(pos) = resp.hover_pos() {
            let row = row_at(pos.y);
            let e = self.entries.at(self.filtered[row]);
            let mut tip = t!("overview.tip.row", row = row + 1, line = e.line_no);
            if e.timestamp().is_some() {
                tip = format!("{tip}\n{}", self.display_dt(&e.datetime()));
            }
            resp.on_hover_text_at_pointer(tip);
        }
    }

    /// View → Overview strip: on / off and the marks shown.
    pub(in crate::app) fn overview_menu(&mut self, ui: &mut egui::Ui) {
        let o = &mut self.cfg.overview;
        let mut changed = ui
            .checkbox(&mut o.enabled, t!("overview.enabled"))
            .on_hover_text(t!("overview.enabled.tip"))
            .changed();
        ui.add_enabled_ui(o.enabled, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                changed |= ui
                    .checkbox(&mut o.errors, t!("severity.error.plural"))
                    .changed();
                changed |= ui
                    .checkbox(&mut o.warnings, t!("severity.warning.plural"))
                    .changed();
                changed |= ui.checkbox(&mut o.search, t!("overview.search")).changed();
                changed |= ui
                    .checkbox(&mut o.bookmarks, t!("bookmarks.title"))
                    .changed();
            });
        });
        if changed {
            self.cfg_dirty = true;
            self.overview_changed_from(0);
        }
    }
}
