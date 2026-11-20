//! The window title (#65): program name and version, then the full path of the log (or the
//! file names of several logs), so that equally named logs from different places can be told
//! apart on the taskbar and in Alt+Tab.

use crate::app::*;
use std::path::Path;

/// The file name of a log: the part after the last `\` or `/` (not `Path::file_name`, so that
/// Windows paths split the same way on every platform and the tests run on Linux, too).
fn file_name(p: &Path) -> String {
    let s = p.to_string_lossy();
    s.rsplit(['\\', '/']).next().unwrap_or_default().to_owned()
}

/// The window title: the program name and version first, then the full path of the log, or the
/// file names of the logs when several are open (`a.log + b.log`; their paths are in the tooltip
/// of the status bar). Several logs get a bracketed prefix before the names: the name of the log
/// set and the number of logs (`[Client install / 3 logs]`), without a set only the number
/// (`[3 logs]`). `count_text` is the translated number of logs, e.g. "3 logs". The title does not
/// follow the selected row.
pub(in crate::app) fn window_title(
    base: &str,
    paths: &[PathBuf],
    set_name: Option<&str>,
    count_text: &str,
) -> String {
    match paths {
        [] => base.to_owned(),
        [one] => format!("{base} – {}", one.display()),
        many => {
            let names: Vec<String> = many.iter().map(|p| file_name(p)).collect();
            let prefix = match set_name.map(str::trim).filter(|s| !s.is_empty()) {
                Some(set) => format!("[{set} / {count_text}]"),
                None => format!("[{count_text}]"),
            };
            format!("{base} – {prefix} {}", names.join(" + "))
        }
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
    fn title_of_nothing_one_log_and_merged_logs() {
        let base = "RusTrace 1.1.9";
        assert_eq!(window_title(base, &[], None, "0 logs"), base);
        // One log: the full path (a log set with one log is shown like a single log).
        assert_eq!(
            window_title(
                base,
                &[p(r"F:\Logs\iis\u_ex261023.log")],
                Some("IIS"),
                "1 logs"
            ),
            r"RusTrace 1.1.9 – F:\Logs\iis\u_ex261023.log"
        );
        assert_eq!(
            window_title(base, &[p(r"\\PC01\c$\Windows\CCM\Logs\a.log")], None, ""),
            r"RusTrace 1.1.9 – \\PC01\c$\Windows\CCM\Logs\a.log"
        );
        // Several logs: the number of logs and the file names, whatever the folders are.
        let many = [
            p(r"C:\Windows\CCM\Logs\ccmexec.log"),
            p(r"\\PC01\c$\Windows\CCM\Logs\PolicyAgent.log"),
            p("rel/dir/c.log"),
        ];
        assert_eq!(
            window_title(base, &many, None, "3 logs"),
            "RusTrace 1.1.9 – [3 logs] ccmexec.log + PolicyAgent.log + c.log"
        );
        // A log set: its name before the number.
        assert_eq!(
            window_title(base, &many, Some("Client install"), "3 logs"),
            "RusTrace 1.1.9 – [Client install / 3 logs] ccmexec.log + PolicyAgent.log + c.log"
        );
        assert_eq!(
            window_title(base, &many, Some("  "), "3 logs"),
            "RusTrace 1.1.9 – [3 logs] ccmexec.log + PolicyAgent.log + c.log"
        );
    }
}
