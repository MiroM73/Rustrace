//! View options from the command line (#26): search, filters and time range set at start-up,
//! the requested row selected once the log has been read.

use crate::app::*;
use crate::cli::ViewOptions;

/// The row the command line asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum CliSelect {
    /// `--line N`: the entry with line N of the first FILE.
    Line(usize),
    /// `--search`: the first matching row.
    FirstMatch,
}

impl App {
    /// Sets the search, filters, time range and following the end; the row to select waits for
    /// the first read of the logs (`has_files`) in `cli_select`. Called once from `App::new`.
    pub(in crate::app) fn apply_cli_view(&mut self, view: ViewOptions, has_files: bool) {
        let searched = view.search.is_some();
        if let Some(text) = view.search {
            self.search_text = text;
            // Also a short text is searched (like after Enter).
            self.search_confirmed = true;
            self.search_regex = view.regex;
            self.search_case = view.case;
            self.filter_mode = view.filter;
        }
        if let Some(s) = view.severity {
            self.show_sev = s.show_sev();
        }
        if let Some(follow) = view.follow {
            self.cfg.auto_scroll = follow;
            self.follow.resume();
        }
        self.time_from = view.from.unwrap_or_default();
        self.time_to = view.to.unwrap_or_default();
        self.apply_time_range();
        self.recompile_search();
        if has_files {
            self.cli_select = match view.line {
                Some(n) => Some(CliSelect::Line(n)),
                None if searched => Some(CliSelect::FirstMatch),
                None => None,
            };
        }
    }

    /// Selects the row asked for by the command line (after the first full read). Like a click
    /// on a row this pauses following the end (#37).
    pub(in crate::app) fn apply_cli_select(&mut self, sel: CliSelect) {
        match sel {
            CliSelect::FirstMatch => self.select_first_match(),
            CliSelect::Line(n) => {
                let file = self.first_log_file();
                let entries = &self.entries;
                // The entry the line belongs to: the last one starting at or before it (an entry
                // can span several lines).
                let found = (0..entries.len()).rev().find(|&i| {
                    file.is_none_or(|f| entries.file(i) == f) && entries.line_no(i) <= n
                });
                match found {
                    Some(i) => {
                        self.after_filter.scroll_end = false;
                        self.select_only(Some(i));
                        self.follow_entry_selected(i);
                        self.scroll_to_selected_after_filter();
                    }
                    None => self.info(t!("cli.line_not_found", line = n)),
                }
            }
        }
    }

    /// File id of the first opened log itself (not its rotated parts); `None` = any file.
    fn first_log_file(&self) -> Option<u16> {
        let first = self.paths.first()?;
        let of_first = |id: usize| self.file_sources.get(id).is_none_or(|&log| log == 0);
        let mut ids = (0..self.files.len()).filter(|&id| of_first(id));
        ids.clone()
            .find(|&id| self.files[id].path.as_ref() == Some(first))
            .or_else(|| ids.next_back())
            .and_then(|id| u16::try_from(id).ok())
    }
}
