//! Selection of several rows (#25): Shift / Ctrl + click and arrows, Ctrl+A, and copying the
//! selected rows (messages, table, original lines).

use crate::app::*;

impl App {
    /// Selects one entry (or nothing): cursor, anchor of Shift ranges and the selection.
    pub(in crate::app) fn select_only(&mut self, i: Option<usize>) {
        self.selected = i;
        self.sel_anchor = i;
        self.sel.clear();
        if let Some(i) = i {
            self.sel.set(i, true);
        }
    }

    /// More than one row is selected.
    pub(in crate::app) fn multi_selected(&self) -> bool {
        self.sel.count() > 1
    }

    /// The row shown in the details pane: the first selected one, or the cursor row.
    pub(in crate::app) fn details_row(&self) -> Option<usize> {
        if self.multi_selected() {
            self.sel.first_one()
        } else {
            self.selected
        }
    }

    /// The selected entries in order (the cursor row when nothing is marked).
    pub(in crate::app) fn selection(&self) -> Vec<usize> {
        if self.sel.count() > 0 {
            self.sel.ones().collect()
        } else {
            self.selected.into_iter().collect()
        }
    }

    /// Position of an entry in the table, or where it would be when it is hidden.
    fn table_pos(&self, i: usize) -> usize {
        self.filtered
            .partition_point(|&f| f < i)
            .min(self.filtered.len().saturating_sub(1))
    }

    /// Selects the displayed rows between the anchor and position `pos` (Shift + click / arrows);
    /// `add` keeps the rows selected before (Ctrl+Shift + click).
    pub(in crate::app) fn extend_selection(&mut self, pos: usize, add: bool) {
        let Some(&idx) = self.filtered.get(pos) else {
            return;
        };
        let anchor = self.sel_anchor.map_or(pos, |a| self.table_pos(a));
        if !add {
            self.sel.clear();
        }
        let (a, b) = (anchor.min(pos), anchor.max(pos));
        for &i in &self.filtered[a..=b] {
            self.sel.set(i, true);
        }
        if self.sel_anchor.is_none() {
            self.sel_anchor = Some(idx);
        }
        self.selected = Some(idx);
        self.scroll_to = Some(pos);
        if pos + 1 < self.filtered.len() {
            self.cfg.auto_scroll = false;
        }
    }

    /// A click on a row: alone, Ctrl = add / remove the row, Shift = range from the anchor,
    /// Ctrl+Shift = add the range (like the lists of Windows Explorer).
    pub(in crate::app) fn click_row(&mut self, idx: usize, mods: Modifiers) {
        if mods.shift {
            if let Ok(pos) = self.filtered.binary_search(&idx) {
                self.extend_selection(pos, mods.command);
                self.scroll_to = None;
            }
        } else if mods.command {
            let on = !self.sel.get(idx);
            // The single selected row becomes part of the selection first.
            if self.sel.count() == 0
                && let Some(s) = self.selected
            {
                self.sel.set(s, true);
            }
            self.sel.set(idx, on);
            self.selected = Some(idx);
            self.sel_anchor = Some(idx);
        } else {
            self.select_only(Some(idx));
        }
    }

    /// Ctrl+A: every row shown in the table.
    pub(in crate::app) fn select_all_visible(&mut self) {
        self.sel.clear();
        for &i in &self.filtered {
            self.sel.set(i, true);
        }
        if self.selected.is_none_or(|s| !self.sel.get(s)) {
            self.selected = self.filtered.first().copied();
            self.sel_anchor = self.selected;
        }
    }

    /// The first `k` entries were removed (memory limit).
    pub(in crate::app) fn selection_drop_front(&mut self, k: usize) {
        self.sel.drain_front(k);
        self.sel_anchor = self.sel_anchor.and_then(|a| a.checked_sub(k));
    }

    /// Ctrl+C: the messages of the selected rows, one after another.
    pub(in crate::app) fn copy_messages(&mut self, ctx: &egui::Context) {
        let rows = self.selection();
        let text: Vec<&str> = rows.iter().map(|&i| self.entries.message(i)).collect();
        ctx.copy_text(text.join("\r\n"));
        self.copied_info(rows.len());
    }

    /// Ctrl+Shift+C: the selected rows tab-separated with the columns of the table (with the
    /// column headers when there are several rows).
    pub(in crate::app) fn copy_table(&mut self, ctx: &egui::Context) {
        let rows = self.selection();
        let columns = self.columns();
        let mut out = String::new();
        if rows.len() > 1 {
            let headers: Vec<String> = std::iter::once(t!("column.line"))
                .chain(columns.iter().map(Self::column_title))
                .collect();
            out.push_str(&headers.join("\t"));
            out.push_str("\r\n");
        }
        for &i in &rows {
            let e = self.entries.at(i);
            out.push_str(&e.line_no.to_string());
            for c in &columns {
                out.push('\t');
                out.push_str(&one_line(&self.cell_text(&e, c.kind)));
            }
            out.push_str("\r\n");
        }
        // A single row without the line break, like before.
        if rows.len() == 1 {
            out.truncate(out.trim_end().len());
        }
        ctx.copy_text(out);
        self.copied_info(rows.len());
    }

    /// The original lines of the selected rows read from their files in a background thread
    /// (rows of overwritten files: their message text).
    pub(in crate::app) fn copy_original(&mut self) {
        let rows = self.selection();
        let mut jobs: Vec<RawJob> = Vec::new();
        let mut lines: Vec<String> = vec![String::new(); rows.len()];
        for (n, &i) in rows.iter().enumerate() {
            let e = self.entries.at(i);
            let file = self
                .files
                .get(e.file as usize)
                .and_then(|f| Some((f.path.clone()?, f.encoding)));
            match (e.raw_range(), file) {
                (Some(range), Some((path, enc))) => {
                    match jobs.iter_mut().find(|(p, _, _)| *p == path) {
                        Some(j) => j.2.push((n, range)),
                        None => jobs.push((path, enc, vec![(n, range)])),
                    }
                }
                _ => lines[n] = e.message.to_owned(),
            }
        }
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        let count = rows.len();
        let spawned = std::thread::Builder::new()
            .name("rustrace-copy".into())
            .spawn(move || {
                let res = jobs
                    .into_iter()
                    .try_for_each(|(path, enc, parts)| {
                        let ranges: Vec<(u64, u64)> = parts.iter().map(|p| p.1).collect();
                        let read = crate::parser::read_raw_many(&path, &ranges, enc)?;
                        for ((n, _), text) in parts.into_iter().zip(read) {
                            lines[n] = text;
                        }
                        Ok::<_, std::io::Error>(())
                    })
                    .map(|_| (lines.join("\r\n"), count))
                    .map_err(|e| e.to_string());
                let _ = tx.send(res);
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.copy_job = Some(rx),
            Err(e) => self.status_error = Some(t!("error.thread", error = e)),
        }
    }

    /// Puts the original lines read in the background on the clipboard.
    pub(in crate::app) fn check_copy_job(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.copy_job else { return };
        match rx.try_recv() {
            Ok(Ok((text, count))) => {
                self.copy_job = None;
                ctx.copy_text(text);
                self.copied_info(count);
            }
            Ok(Err(e)) => {
                self.copy_job = None;
                self.status_error = Some(t!("error.copy", error = e));
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => self.copy_job = None,
        }
    }

    fn copied_info(&mut self, rows: usize) {
        if rows > 1 {
            self.info(t!("selection.copied", count = rows));
        }
    }

    /// "N rows selected" and the time between the first and the last selected row.
    pub(in crate::app) fn selection_summary(&self) -> Option<String> {
        if !self.multi_selected() {
            return None;
        }
        let count = self.sel.count();
        let ts = |i: Option<usize>| i.and_then(|i| self.entries.get(i)?.timestamp());
        Some(match (ts(self.sel.first_one()), ts(self.sel.last_one())) {
            (Some(a), Some(b)) => t!(
                "selection.summary.span",
                count = count,
                span = crate::timefmt::format_duration(b - a)
            ),
            _ => t!("selection.summary", count = count),
        })
    }
}

/// Original lines read from one file: path, encoding, (row number in the result, byte range).
type RawJob = (PathBuf, crate::parser::Encoding, Vec<(usize, (u64, u64))>);

/// A cell text on one line (multi-line messages: " ↵ " between the lines).
fn one_line(s: &str) -> String {
    if s.contains('\n') {
        s.replace("\r\n", "\n").replace('\n', " ↵ ")
    } else {
        s.to_owned()
    }
}
