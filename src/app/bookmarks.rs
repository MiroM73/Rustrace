//! Bookmarks (#22): marked rows, kept by (file id, line number), so they stay on the same log
//! line through refreshes, filtering, merges of older rows and the removal of old records.

use crate::app::*;
use std::collections::HashSet;

impl App {
    /// (file id, line number) of an entry – what a bookmark is kept by.
    pub(in crate::app) fn bookmark_key(&self, i: usize) -> (u16, usize) {
        (self.entries.file(i), self.entries.line_no(i))
    }

    pub(in crate::app) fn is_bookmarked(&self, i: usize) -> bool {
        self.bookmark_rows.binary_search(&i).is_ok()
    }

    /// Note of a bookmarked entry (`None` = not bookmarked).
    pub(in crate::app) fn bookmark_note(&self, i: usize) -> Option<&str> {
        if !self.is_bookmarked(i) {
            return None;
        }
        self.bookmarks
            .get(&self.bookmark_key(i))
            .map(String::as_str)
    }

    pub(in crate::app) fn set_bookmark_note(&mut self, i: usize, note: String) {
        let key = self.bookmark_key(i);
        if let Some(n) = self.bookmarks.get_mut(&key) {
            *n = note;
            self.bookmarks_gen += 1;
        }
    }

    /// Adds or removes the bookmark of an entry (Ctrl+F2, context menu).
    pub(in crate::app) fn toggle_bookmark(&mut self, i: usize) {
        if i >= self.entries.len() {
            return;
        }
        let key = self.bookmark_key(i);
        match self.bookmark_rows.binary_search(&i) {
            Ok(pos) => {
                self.bookmark_rows.remove(pos);
                self.bookmarks.remove(&key);
            }
            Err(pos) => {
                self.bookmark_rows.insert(pos, i);
                self.bookmarks.insert(key, String::new());
            }
        }
        self.bookmarks_changed();
    }

    pub(in crate::app) fn remove_bookmark(&mut self, i: usize) {
        if self.is_bookmarked(i) {
            self.toggle_bookmark(i);
        }
    }

    pub(in crate::app) fn clear_bookmarks(&mut self) {
        self.bookmarks.clear();
        self.bookmark_rows.clear();
        self.bookmarks_changed();
    }

    fn bookmarks_changed(&mut self) {
        self.bookmarks_gen += 1;
        if self.bookmarks_only {
            self.refilter_from(0);
        }
    }

    /// The bookmarked entries must be looked up again from entry `start` on (new or merged rows).
    pub(in crate::app) fn find_bookmarks_from(&mut self, start: usize) {
        let keep = self.bookmark_rows.partition_point(|&i| i < start);
        self.bookmark_rows.truncate(keep);
        if self.bookmarks.is_empty() {
            return;
        }
        for i in start..self.entries.len() {
            if self.bookmarks.contains_key(&self.bookmark_key(i)) {
                self.bookmark_rows.push(i);
            }
        }
    }

    /// After the whole view was read again: bookmarks follow their files to the new file ids,
    /// bookmarks of files or lines that are no longer there are removed.
    pub(in crate::app) fn bookmarks_after_reset(&mut self, old_files: &[SourceFile]) {
        if !self.bookmarks.is_empty() {
            let map = remap_files(old_files, &self.files);
            self.bookmarks = std::mem::take(&mut self.bookmarks)
                .into_iter()
                .filter_map(|((f, line), note)| {
                    let f = map.get(f as usize).copied().flatten()?;
                    Some(((f, line), note))
                })
                .collect();
        }
        self.find_bookmarks_from(0);
        let found: HashSet<(u16, usize)> = self
            .bookmark_rows
            .iter()
            .map(|&i| self.bookmark_key(i))
            .collect();
        self.bookmarks.retain(|k, _| found.contains(k));
        self.bookmarks_gen += 1;
    }

    /// The first `k` entries were removed (memory limit): their bookmarks go with them.
    pub(in crate::app) fn bookmarks_drop_front(&mut self, k: usize) {
        let gone = self.bookmark_rows.partition_point(|&i| i < k);
        if gone > 0 {
            let keys: Vec<(u16, usize)> = self.bookmark_rows[..gone]
                .iter()
                .map(|&i| self.bookmark_key(i))
                .collect();
            for key in keys {
                self.bookmarks.remove(&key);
            }
            self.bookmark_rows.drain(..gone);
            self.bookmarks_gen += 1;
        }
        for i in &mut self.bookmark_rows {
            *i -= k;
        }
    }

    /// The keys for the "only bookmarked rows" filter.
    pub(in crate::app) fn bookmark_filter(&self) -> Option<Arc<HashSet<(u16, usize)>>> {
        self.bookmarks_only
            .then(|| Arc::new(self.bookmarks.keys().copied().collect()))
    }

    /// F2 / Shift+F2: the next / previous bookmarked row shown in the table (wraps around).
    pub(in crate::app) fn goto_bookmark(&mut self, forward: bool) {
        let shown: Vec<usize> = self
            .bookmark_rows
            .iter()
            .filter_map(|i| self.filtered.binary_search(i).ok())
            .collect();
        if shown.is_empty() {
            self.info(t!(if self.bookmarks.is_empty() {
                "bookmarks.none"
            } else {
                "bookmarks.none_shown"
            }));
            return;
        }
        // Position of the selected row in the table, or where it would be when it is hidden.
        let cur = self.selected.map(|s| {
            let p = self.filtered.partition_point(|&i| i < s);
            (p, self.filtered.get(p) == Some(&s))
        });
        let last = shown[shown.len() - 1];
        let pos = match cur {
            None if forward => shown[0],
            None => last,
            Some((p, on_row)) if forward => {
                let next = shown.partition_point(|&b| b < p + usize::from(on_row));
                shown.get(next).copied().unwrap_or(shown[0])
            }
            Some((p, _)) => {
                let prev = shown.partition_point(|&b| b < p);
                prev.checked_sub(1).map_or(last, |q| shown[q])
            }
        };
        self.select_row(pos);
    }

    /// Selects an entry and scrolls to it (bookmarks window); a row hidden by the filters is
    /// only selected.
    pub(in crate::app) fn jump_to_entry(&mut self, i: usize) {
        match self.filtered.binary_search(&i) {
            Ok(pos) => self.select_row(pos),
            Err(_) => {
                self.selected = Some(i);
                self.info(t!("bookmarks.hidden"));
            }
        }
    }

    /// Bookmarks for the program restarted after an update.
    pub(in crate::app) fn bookmark_state(&self) -> Vec<(u16, usize, String)> {
        let mut v: Vec<(u16, usize, String)> = self
            .bookmarks
            .iter()
            .map(|(&(f, l), n)| (f, l, n.clone()))
            .collect();
        v.sort();
        v
    }

    pub(in crate::app) fn restore_bookmarks(&mut self, list: Vec<(u16, usize, String)>) {
        if list.is_empty() {
            return;
        }
        self.bookmarks = list.into_iter().map(|(f, l, n)| ((f, l), n)).collect();
        self.find_bookmarks_from(0);
        self.bookmarks_changed();
    }
}

/// New file id of every old file (index = old id): the same path, or the same name for a part
/// overwritten by a rotation; `None` when the file is no longer in the view.
fn remap_files(old: &[SourceFile], new: &[SourceFile]) -> Vec<Option<u16>> {
    old.iter()
        .map(|o| {
            new.iter()
                .position(|n| match (&o.path, &n.path) {
                    (Some(a), Some(b)) => a == b,
                    (None, None) => o.name == n.name,
                    _ => false,
                })
                .map(|p| p as u16)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Encoding;

    fn file(name: &str, path: Option<&str>) -> SourceFile {
        SourceFile {
            name: name.into(),
            path: path.map(PathBuf::from),
            encoding: Encoding::Utf8,
        }
    }

    #[test]
    fn files_are_remapped_by_path() {
        let old = [
            file("a.lo_", Some("C:/l/a.lo_")),
            file("a.log", Some("C:/l/a.log")),
            file("b.log", Some("C:/l/b.log")),
            file("x.log", None),
        ];
        // Another log was added in front, b.log removed.
        let new = [
            file("c.log", Some("C:/l/c.log")),
            file("a.lo_", Some("C:/l/a.lo_")),
            file("a.log", Some("C:/l/a.log")),
            file("x.log", None),
        ];
        assert_eq!(remap_files(&old, &new), [Some(1), Some(2), None, Some(3)]);
        assert_eq!(remap_files(&old, &[]), [None, None, None, None]);
    }
}
