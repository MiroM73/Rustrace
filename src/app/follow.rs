//! "Follow end" paused while a row is selected (#37).
//!
//! `cfg.auto_scroll` is the user's setting (toolbar / View menu checkbox, `--tail` /
//! `--no-follow`, saved). Selecting or jumping to a row that is not the last one shown only
//! **pauses** following (runtime state, not saved): the table stays at the row while new rows
//! arrive below it. Following resumes when the selection is cleared (Esc, Ctrl+click on the only
//! selected row, a click below the last row), when the last row is reached (End, a click on it,
//! ↓ / PgDn onto it, scrolling the table to the very bottom) or when the checkbox is ticked.

use crate::app::*;

/// The runtime part of following the end: paused or not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct Follow {
    paused: bool,
}

impl Follow {
    /// The table follows the end: the setting is on and following is not paused.
    pub(in crate::app) fn active(self, setting: bool) -> bool {
        setting && !self.paused
    }

    /// Following is paused (shown as "Follow end (paused)" when the setting is on).
    pub(in crate::app) fn paused(self) -> bool {
        self.paused
    }

    /// A row at position `pos` of the `len` rows shown was selected or jumped to: a row above
    /// the last one pauses following, the last row resumes it.
    pub(in crate::app) fn row_selected(&mut self, pos: usize, len: usize) {
        self.paused = pos + 1 < len;
    }

    /// A row that is not shown (hidden by the filters) was selected: following pauses.
    pub(in crate::app) fn pause(&mut self) {
        self.paused = true;
    }

    /// The selection was cleared, the end was reached or the checkbox was changed; `true` when
    /// following was paused before.
    pub(in crate::app) fn resume(&mut self) -> bool {
        std::mem::take(&mut self.paused)
    }
}

impl App {
    /// The table follows the end of the log now (setting on and not paused).
    pub(in crate::app) fn following(&self) -> bool {
        self.follow.active(self.cfg.auto_scroll)
    }

    /// Following is on as a setting but paused while a row is selected.
    pub(in crate::app) fn follow_paused(&self) -> bool {
        self.cfg.auto_scroll && self.follow.paused()
    }

    /// The row at position `pos` of the table was selected / jumped to.
    pub(in crate::app) fn follow_row_selected(&mut self, pos: usize) {
        self.follow.row_selected(pos, self.filtered.len());
    }

    /// Entry `i` was selected: like `follow_row_selected` for its row, or a pause when the row is
    /// hidden by the filters.
    pub(in crate::app) fn follow_entry_selected(&mut self, i: usize) {
        match self.filtered.binary_search(&i) {
            Ok(pos) => self.follow_row_selected(pos),
            Err(_) => self.follow.pause(),
        }
    }

    /// Resumes a paused following and scrolls to the end.
    pub(in crate::app) fn resume_follow(&mut self) {
        if self.follow.resume() && self.cfg.auto_scroll {
            self.scroll_to_end_after_filter();
        }
    }

    /// The "Follow end" checkbox (toolbar, View menu) was changed: the setting is saved and a
    /// pause ends; ticking it scrolls to the end (the selection may stay).
    pub(in crate::app) fn follow_setting_changed(&mut self) {
        self.cfg_dirty = true;
        self.follow.resume();
        if self.cfg.auto_scroll {
            self.scroll_to_end_after_filter();
        }
    }

    /// Clears the selection (Esc, …); a paused following resumes.
    pub(in crate::app) fn clear_selection(&mut self) {
        self.select_only(None);
        self.resume_follow();
    }

    /// Label of the "Follow end" checkbox: with "(paused)" while following is paused.
    pub(in crate::app) fn follow_label(&self, key: &str) -> String {
        if self.follow_paused() {
            t!("toolbar.follow.paused", label = t!(key))
        } else {
            t!(key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_end_pauses_and_resumes() {
        let mut f = Follow::default();
        assert!(f.active(true));
        // A row in the middle pauses following, the setting stays on.
        f.row_selected(10, 100);
        assert!(f.paused() && !f.active(true));
        // The last row resumes it.
        f.row_selected(99, 100);
        assert!(!f.paused() && f.active(true));
        // Clearing the selection (or ticking the checkbox) resumes it.
        f.row_selected(0, 100);
        assert!(f.resume());
        assert!(f.active(true));
        assert!(!f.resume(), "nothing to resume");
        // A row hidden by the filters pauses too.
        f.pause();
        assert!(!f.active(true));
        // Unticked: off whatever the pause says (no automatic resume).
        f.resume();
        assert!(!f.active(false));
        f.row_selected(99, 100);
        assert!(!f.active(false));
        // A single row is the last one.
        f.row_selected(0, 1);
        assert!(f.active(true));
    }
}
