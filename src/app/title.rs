//! The window title (#65): the file name and its folder, so that equally named logs from
//! different places can be told apart on the taskbar and in Alt+Tab.

use crate::app::*;
use std::path::Path;

/// Splits a path into folder and file name at the last `\` or `/` (not `Path::parent`, so that
/// Windows paths split the same way on every platform and the tests run on Linux, too).
/// The folder is "" for a bare file name.
fn split_path(p: &Path) -> (String, String) {
    let s = p.to_string_lossy();
    match s.rfind(['\\', '/']) {
        Some(i) => (s[..i].to_owned(), s[i + 1..].to_owned()),
        None => (String::new(), s.into_owned()),
    }
}

fn folder(p: &Path) -> String {
    split_path(p).0
}

fn file_name(p: &Path) -> String {
    split_path(p).1
}

/// The window title. The file name comes first because Windows truncates long titles from the
/// right; the title does not follow the selected row.
/// - nothing open: `base`;
/// - one log: `name – folder – base`;
/// - several logs of a log set: `set name (count_text) – base`;
/// - several logs in one folder: `a + b – folder – base`;
/// - several logs in different folders: `a + b (count_text) – base`.
///
/// `count_text` is the translated number of logs, e.g. "2 logs".
pub(in crate::app) fn window_title(
    base: &str,
    paths: &[PathBuf],
    set_name: Option<&str>,
    count_text: &str,
) -> String {
    let Some(first) = paths.first() else {
        return base.to_owned();
    };
    let names = paths
        .iter()
        .map(|p| file_name(p))
        .collect::<Vec<_>>()
        .join(" + ");
    let dir = folder(first);
    let with_folder = |names: &str| {
        if dir.is_empty() {
            format!("{names} – {base}")
        } else {
            format!("{names} – {dir} – {base}")
        }
    };
    if paths.len() == 1 {
        return with_folder(&names);
    }
    if let Some(set) = set_name.filter(|s| !s.trim().is_empty()) {
        return format!("{set} ({count_text}) – {base}");
    }
    let same_folder = paths
        .iter()
        .all(|p| folder(p).to_lowercase() == dir.to_lowercase());
    if same_folder {
        with_folder(&names)
    } else {
        format!("{names} ({count_text}) – {base}")
    }
}

/// Most selected rows looked at when searching the log of the selection (a bigger selection
/// is not scanned in every frame).
const MAX_SELECTION_SCAN: usize = 100_000;

impl App {
    pub(super) fn update_title(&mut self, ctx: &egui::Context) {
        let t = window_title(
            &base_title(),
            &self.paths,
            self.set_name.as_deref(),
            &t!("title.logs", count = self.paths.len()),
        );
        if t != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(t.clone()));
            self.title = t;
        }
    }

    /// The opened log (index into `paths`) all selected rows belong to; None when nothing is
    /// selected, the rows come from several logs or the selection is too big to look at.
    pub(in crate::app) fn selection_source(&self) -> Option<usize> {
        let source_of = |row: usize| {
            self.entries.get(row)?;
            self.file_sources
                .get(self.entries.file(row) as usize)
                .copied()
        };
        let mut found = None;
        let rows: Box<dyn Iterator<Item = usize> + '_> = if self.sel.count() > 0 {
            if self.sel.count() > MAX_SELECTION_SCAN {
                return None;
            }
            Box::new(self.sel.ones())
        } else {
            Box::new(self.selected.into_iter())
        };
        for row in rows {
            let s = source_of(row)?;
            if *found.get_or_insert(s) != s {
                return None;
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn title_of_nothing_and_of_one_log() {
        assert_eq!(
            window_title("RusTrace 1.2.0", &[], None, "0 logs"),
            "RusTrace 1.2.0"
        );
        assert_eq!(
            window_title(
                "RusTrace 1.2.0",
                &[p(r"C:\Windows\CCM\Logs\ccmexec.log")],
                None,
                "1 logs"
            ),
            r"ccmexec.log – C:\Windows\CCM\Logs – RusTrace 1.2.0"
        );
        // A bare file name has no folder to show.
        assert_eq!(
            window_title("RusTrace", &[p("a.log")], None, ""),
            "a.log – RusTrace"
        );
        // UNC paths stay as they are.
        assert_eq!(
            window_title(
                "RusTrace",
                &[p(r"\\PC01\c$\Windows\CCM\Logs\a.log")],
                None,
                ""
            ),
            r"a.log – \\PC01\c$\Windows\CCM\Logs – RusTrace"
        );
    }

    #[test]
    fn title_of_merged_logs() {
        let same = [
            p(r"C:\Windows\CCM\Logs\ccmexec.log"),
            p(r"c:\windows\ccm\logs\PolicyAgent.log"),
        ];
        assert_eq!(
            window_title("RusTrace", &same, None, "2 logs"),
            r"ccmexec.log + PolicyAgent.log – C:\Windows\CCM\Logs – RusTrace"
        );
        let different = [
            p(r"C:\Windows\CCM\Logs\ccmexec.log"),
            p(r"\\PC01\c$\Windows\CCM\Logs\ccmexec.log"),
        ];
        assert_eq!(
            window_title("RusTrace", &different, None, "2 logs"),
            "ccmexec.log + ccmexec.log (2 logs) – RusTrace"
        );
        // A log set: its name and the number of logs, whatever the folders are.
        assert_eq!(
            window_title("RusTrace", &same, Some("Client install"), "2 logs"),
            "Client install (2 logs) – RusTrace"
        );
        // A set with one log is shown like a single log.
        assert_eq!(
            window_title("RusTrace", &same[..1], Some("Client install"), "1 logs"),
            r"ccmexec.log – C:\Windows\CCM\Logs – RusTrace"
        );
    }
}
