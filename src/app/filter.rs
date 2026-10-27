//! Search, filter mode, severity toggles, time range and column filters.

use crate::app::*;

impl App {
    pub(in crate::app) fn recompile_rules(&mut self) {
        self.rules_gen += 1;
        self.rules.clear();
        self.rule_errors.clear();
        for r in &self.cfg.rules {
            if r.pattern.is_empty() {
                self.rule_errors.push(None);
                continue;
            }
            match compile_pattern(&r.pattern, r.is_regex, r.case_sensitive) {
                Ok(re) => {
                    self.rule_errors.push(None);
                    if r.enabled {
                        self.rules.push(CompiledRule {
                            regex: re,
                            rule: r.clone(),
                        });
                    }
                }
                Err(e) => self.rule_errors.push(Some(e)),
            }
        }
    }

    pub(in crate::app) fn recompile_search(&mut self) {
        self.search_gen += 1;
        self.search_err = None;
        self.search_re = None;
        if self.search_active() {
            match compile_pattern(&self.search_text, self.search_regex, self.search_case) {
                Ok(re) => self.search_re = Some(re),
                Err(e) => self.search_err = Some(e),
            }
        }
        self.refilter_from(0);
    }

    /// The search text is searched: typed texts from `INSTANT_SEARCH_MIN_CHARS` characters on,
    /// shorter ones only after an explicit search (#4).
    pub(in crate::app) fn search_active(&self) -> bool {
        search_active(&self.search_text, self.search_confirmed)
    }

    /// Selects the first displayed row that matches the search (search as you type); while a
    /// background re-filter runs, once its result is there.
    pub(in crate::app) fn select_first_match(&mut self) {
        if self.filter_job.is_some() {
            self.after_filter.select_first = true;
            return;
        }
        if let Some(pos) = self.filtered.iter().position(|&i| self.is_match.get(i)) {
            self.select_row(pos);
        }
    }

    /// Follows the end of the log (now, or after the running background re-filter).
    pub(in crate::app) fn scroll_to_end_after_filter(&mut self) {
        if self.filter_job.is_some() {
            self.after_filter.scroll_end = true;
        } else if !self.filtered.is_empty() {
            self.scroll_to = Some(self.filtered.len() - 1);
        }
    }

    /// Scrolls to the selected row (now, or after the running background re-filter).
    pub(in crate::app) fn scroll_to_selected_after_filter(&mut self) {
        if self.filter_job.is_some() {
            self.after_filter.scroll_selected = true;
        } else if let Some(pos) = self
            .selected
            .and_then(|s| self.filtered.binary_search(&s).ok())
        {
            self.scroll_to = Some(pos);
        }
    }

    /// Text of a column for an entry (date/time formatted for display).
    pub(in crate::app) fn cell_text(&self, e: &EntryRef, kind: ColumnKind) -> String {
        match kind {
            ColumnKind::Role(Role::Message) => e.message.to_owned(),
            ColumnKind::Role(Role::Component) => e.component.to_owned(),
            ColumnKind::Role(Role::DateTime) => self.display_dt(&e.datetime()),
            ColumnKind::Role(Role::Thread) => e.thread.to_owned(),
            ColumnKind::Extra(i) => e.extra(i).to_owned(),
            ColumnKind::File => self.file_label(e.file),
            // Needs the row position: `App::cell` / `App::delta_cell`.
            ColumnKind::Delta => String::new(),
        }
    }

    /// Everything the filter depends on, for the filter run.
    fn filter_spec(&self) -> FilterSpec {
        FilterSpec {
            search: self.search_re.clone(),
            filter_mode: self.filter_mode,
            show_sev: self.show_sev,
            time_range: self.time_range,
            columns: self.col_filters.clone(),
            file_labels: self.file_labels.clone(),
            bookmarks: self.bookmark_filter(),
        }
    }

    /// Recomputes the filter from entry `start` on (earlier entries stay). A full re-filter of a
    /// big log runs in a background thread (#16); until its result is there the previous one
    /// stays displayed. Appended rows and small logs are filtered right away.
    pub(in crate::app) fn refilter_from(&mut self, start: usize) {
        // A running full re-filter is outdated now – start over from the beginning.
        let restart = self.cancel_filter_job() | std::mem::take(&mut self.filter_restart);
        let start = if restart { 0 } else { start };
        let spec = self.filter_spec();
        if start == 0 && self.entries.len() > SYNC_FILTER_MAX_ENTRIES {
            self.start_filter_job(spec);
        } else {
            self.refilter_here(start, &spec);
        }
    }

    /// Filters the entries from `start` on this thread.
    fn refilter_here(&mut self, start: usize, spec: &FilterSpec) {
        let pos = self.filtered.partition_point(|&i| i < start);
        self.overview_changed_from(pos);
        while self.filtered.last().is_some_and(|&i| i >= start) {
            self.filtered.pop();
        }
        self.is_match.truncate(start);
        let mut out = FilterOutput {
            filtered: std::mem::take(&mut self.filtered),
            is_match: std::mem::take(&mut self.is_match),
        };
        spec.run(&self.entries, start..self.entries.len(), &mut out);
        self.filter_done(out);
    }

    /// Starts the full re-filter in a background thread.
    fn start_filter_job(&mut self, spec: FilterSpec) {
        let entries = self.entries.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        let flag = cancel.clone();
        let spawned = std::thread::Builder::new()
            .name("rustrace-filter".into())
            .spawn(move || {
                let out = spec.run_parallel(&entries, &flag);
                // Released before the UI hears about it: it may need the store to itself.
                drop(entries);
                if out.is_some() {
                    let _ = tx.send(out);
                    ctx.request_repaint();
                }
            });
        match spawned {
            Ok(handle) => {
                self.filter_job = Some(FilterJob {
                    rx,
                    cancel,
                    handle,
                    started: Instant::now(),
                });
            }
            Err(_) => self.refilter_here(0, &self.filter_spec()),
        }
    }

    /// Takes over the result of the background re-filter when it is ready.
    pub(in crate::app) fn check_filter_job(&mut self) {
        self.stale_filter_jobs.retain(|h| !h.is_finished());
        let Some(job) = &self.filter_job else { return };
        match job.rx.try_recv() {
            Ok(Some(out)) => {
                self.filter_job = None;
                self.overview_changed_from(0);
                self.filter_done(out);
            }
            Ok(None) | Err(mpsc::TryRecvError::Disconnected) => {
                // The thread crashed – filter here instead.
                self.filter_job = None;
                self.refilter_here(0, &self.filter_spec());
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Stops the running background re-filter; `true` when there was one.
    pub(in crate::app) fn cancel_filter_job(&mut self) -> bool {
        let Some(job) = self.filter_job.take() else {
            return false;
        };
        job.cancel.store(true, Ordering::Relaxed);
        // It stops at the next chunk; the store may be changed only after it let go of it.
        self.stale_filter_jobs.push(job.handle);
        true
    }

    /// The entries for changing them in place. A running background re-filter is stopped first
    /// (the next `refilter_from` starts it again from the beginning).
    pub(in crate::app) fn entries_mut(&mut self) -> &mut LogStore {
        if self.cancel_filter_job() {
            self.filter_restart = true;
        }
        for h in self.stale_filter_jobs.drain(..) {
            let _ = h.join();
        }
        Arc::get_mut(&mut self.entries).expect("no filter thread holds the entries")
    }

    /// How long the background re-filter has been running (`None` = none runs).
    pub(in crate::app) fn filter_running_for(&self) -> Option<Duration> {
        self.filter_job.as_ref().map(|j| j.started.elapsed())
    }

    /// Displays a filter result and does what waited for it.
    fn filter_done(&mut self, out: FilterOutput) {
        self.filtered = out.filtered;
        self.is_match = out.is_match;
        debug_assert_eq!(self.is_match.count(), self.is_match.recount());
        debug_assert_eq!(self.is_match.len(), self.entries.len());
        if let Some(s) = self.selected
            && s >= self.entries.len()
        {
            self.selected = None;
        }
        self.sel.truncate(self.entries.len());
        let after = std::mem::take(&mut self.after_filter);
        if after.select_first {
            self.select_first_match();
        }
        if after.scroll_end {
            self.scroll_to_end_after_filter();
        }
        if after.scroll_selected && !self.cfg.auto_scroll {
            self.scroll_to_selected_after_filter();
        }
    }

    pub(in crate::app) fn time_range_active(&self) -> bool {
        self.time_range != (None, None)
    }

    /// Parses the from / to texts and filters the table again.
    pub(in crate::app) fn apply_time_range(&mut self) {
        let parse = |text: &str, end: bool| {
            if text.trim().is_empty() {
                (None, false)
            } else {
                let b = export::parse_range_bound(text, &self.date_format, end);
                (b, b.is_none())
            }
        };
        let (from, from_bad) = parse(&self.time_from, false);
        let (to, to_bad) = parse(&self.time_to, true);
        self.time_invalid = (from_bad, to_bad);
        if (from, to) != self.time_range {
            self.time_range = (from, to);
            self.refilter_from(0);
        }
    }

    /// Compiles the column filter texts and filters the table again. The date/time column is
    /// compared with the timestamps where possible (#17).
    pub(in crate::app) fn apply_column_filters(&mut self) {
        self.col_filters = self
            .columns()
            .iter()
            .filter_map(|c| {
                let text = self.col_filter_text.get(&c.kind)?;
                let f = match c.kind {
                    ColumnKind::Role(Role::DateTime) => {
                        CellFilter::Date(DateFilter::parse(text, &self.date_format)?)
                    }
                    _ => CellFilter::Text(ColumnFilter::parse(text)?),
                };
                Some((c.kind, f))
            })
            .collect();
        self.refilter_from(0);
    }

    /// Formats an ISO date-time ("2026-10-23 13:22:01.123") using the language pattern.
    pub(in crate::app) fn display_dt(&self, iso: &str) -> String {
        self.date_layout.format_iso(iso)
    }

    /// Adds the current search text to the search history.
    pub(in crate::app) fn remember_search(&mut self) {
        let text = self.search_text.clone();
        if self.cfg.push_search(&text) {
            self.cfg_dirty = true;
        }
    }

    pub(in crate::app) fn find(&mut self, forward: bool) {
        self.remember_search();
        // A short text typed without searching yet is searched now.
        if self.search_re.is_none() && self.search_err.is_none() && !self.search_text.is_empty() {
            self.search_confirmed = true;
            self.recompile_search();
        }
        let Some(re) = self.search_re.clone() else {
            self.focus_search = true;
            return;
        };
        let n = self.filtered.len();
        if n == 0 {
            return;
        }
        let cur = self
            .selected
            .and_then(|s| self.filtered.binary_search(&s).ok());
        for k in 1..=n {
            let pos = match cur {
                Some(p) if forward => (p + k) % n,
                Some(p) => (p + n - (k % n)) % n,
                None if forward => k - 1,
                None => n - k,
            };
            let idx = self.filtered[pos];
            if entry_matches(&re, &self.entries.at(idx)) {
                self.select_row(pos);
                return;
            }
        }
        self.info(t!("search.not_found"));
    }

    /// Selects a row by its position in the displayed (filtered) list.
    pub(in crate::app) fn select_row(&mut self, pos: usize) {
        if let Some(&idx) = self.filtered.get(pos) {
            self.select_only(Some(idx));
            self.scroll_to = Some(pos);
            // Manual navigation stops following the end, otherwise the table would jump.
            if pos + 1 < self.filtered.len() {
                self.cfg.auto_scroll = false;
            }
        }
    }
}

/// A search text is searched: non-empty and either long enough to search as you type or
/// searched explicitly.
fn search_active(text: &str, confirmed: bool) -> bool {
    !text.is_empty() && (confirmed || text.chars().count() >= INSTANT_SEARCH_MIN_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_starts_from_three_characters() {
        assert!(!search_active("", false));
        assert!(!search_active("", true));
        assert!(!search_active("ab", false));
        assert!(search_active("ab", true));
        assert!(search_active("abc", false));
        // Characters, not bytes.
        assert!(!search_active("čš", false));
        assert!(search_active("čšž", false));
    }
}
