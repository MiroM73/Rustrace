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

    /// Selects the first displayed row that matches the search (search as you type).
    pub(in crate::app) fn select_first_match(&mut self) {
        if let Some(pos) = self.filtered.iter().position(|&i| self.is_match.get(i) == Some(&true)) {
            self.select_row(pos);
        }
    }

    pub(in crate::app) fn entry_matches(re: &Regex, e: &EntryRef) -> bool {
        re.is_match(e.message) || Self::other_columns_match(re, e)
    }

    /// Component or one of the extra columns of a log definition matches.
    pub(in crate::app) fn other_columns_match(re: &Regex, e: &EntryRef) -> bool {
        re.is_match(e.component) || e.extras().any(|v| re.is_match(v))
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
        }
    }

    pub(in crate::app) fn sev_visible(&self, s: Severity) -> bool {
        match s {
            Severity::Info => self.show_sev[0],
            Severity::Warning => self.show_sev[1],
            Severity::Error => self.show_sev[2],
        }
    }

    /// Recomputes the filter from entry `start` on (earlier entries stay).
    pub(in crate::app) fn refilter_from(&mut self, start: usize) {
        while self.filtered.last().is_some_and(|&i| i >= start) {
            self.filtered.pop();
        }
        self.is_match.truncate(start);
        for i in start..self.entries.len() {
            let e = self.entries.at(i);
            let m = self
                .search_re
                .as_ref()
                .is_some_and(|re| Self::entry_matches(re, &e));
            self.is_match.push(m);
            let pass_search = !self.filter_mode || self.search_re.is_none() || m;
            if pass_search
                && self.sev_visible(e.severity)
                && self.in_time_range(e.timestamp())
                && self.passes_column_filters(&e)
            {
                self.filtered.push(i);
            }
        }
        self.match_count = self.is_match.iter().filter(|&&b| b).count();
        if let Some(s) = self.selected
            && s >= self.entries.len()
        {
            self.selected = None;
        }
    }

    /// All column filters match the entry (the date/time as displayed).
    pub(in crate::app) fn passes_column_filters(&self, e: &EntryRef) -> bool {
        self.col_filters.iter().all(|(kind, f)| match kind {
            ColumnKind::Role(Role::Message) => f.matches(e.message),
            ColumnKind::Role(Role::Component) => f.matches(e.component),
            ColumnKind::Role(Role::Thread) => f.matches(e.thread),
            ColumnKind::Role(Role::DateTime) => f.matches(&self.display_dt(&e.datetime())),
            ColumnKind::Extra(i) => f.matches(e.extra(*i)),
            ColumnKind::File => f.matches(self.file_labels.get(e.file as usize).map_or("", String::as_str)),
        })
    }

    /// The time range filter lets the entry through. With an active range, entries without
    /// a date/time are hidden (their time is unknown).
    pub(in crate::app) fn in_time_range(&self, ts: Option<i64>) -> bool {
        match self.time_range {
            (None, None) => true,
            (from, to) => ts.is_some_and(|t| from.is_none_or(|f| t >= f) && to.is_none_or(|x| t <= x)),
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
            self.filtered.clear();
            self.refilter_from(0);
        }
    }

    /// Compiles the column filter texts and filters the table again.
    pub(in crate::app) fn apply_column_filters(&mut self) {
        self.col_filters = self
            .columns()
            .iter()
            .filter_map(|c| {
                let text = self.col_filter_text.get(&c.kind)?;
                ColumnFilter::parse(text).map(|f| (c.kind, f))
            })
            .collect();
        self.filtered.clear();
        self.refilter_from(0);
    }

    /// Formats an ISO date-time ("2026-10-23 13:22:01.123") using the language pattern.
    pub(in crate::app) fn display_dt(&self, iso: &str) -> String {
        let b = iso.as_bytes();
        if b.len() >= 10 && b[4] == b'-' && b[7] == b'-' {
            let date = self
                .date_format
                .replace("{y}", &iso[0..4])
                .replace("{m}", &iso[5..7])
                .replace("{d}", &iso[8..10]);
            format!("{date}{}", &iso[10..])
        } else {
            iso.to_owned()
        }
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
            if Self::entry_matches(&re, &self.entries.at(idx)) {
                self.select_row(pos);
                return;
            }
        }
        self.info(t!("search.not_found"));
    }

    /// Selects a row by its position in the displayed (filtered) list.
    pub(in crate::app) fn select_row(&mut self, pos: usize) {
        if let Some(&idx) = self.filtered.get(pos) {
            self.selected = Some(idx);
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
