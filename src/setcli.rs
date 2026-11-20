//! `--set <NAME|FILE.set>` (#67): finds a log set and resolves its logs like File → Open log set.
//!
//! [`find`] and [`paths`] are pure (given the loaded sets and variables); [`open`] adds the
//! loading of the sets, the variables of this / a remote computer and the check of every file,
//! and returns either the paths to open or one message for the window / stderr.

use std::path::{Path, PathBuf};

use crate::logset::{self, FileState, LogSetDef, SetList, Vars};
use crate::t;

/// The set a `--set` value names: an existing `.set` file, else the file name or the name (any
/// language) of a built-in or own set – own sets first, so a user's set wins.
pub fn find(value: &str, list: &SetList) -> Result<LogSetDef, String> {
    let as_path = Path::new(value.trim());
    let is_file = as_path.is_file();
    let looks_like_file = value.trim().to_lowercase().ends_with(".set");
    if is_file || (looks_like_file && as_path.components().count() > 1) {
        let text = std::fs::read_to_string(as_path)
            .map_err(|e| t!("cli.error.set_file", file = as_path.display(), error = e))?;
        let id = as_path
            .file_name()
            .map_or_else(|| value.to_owned(), |n| n.to_string_lossy().into_owned());
        return LogSetDef::parse(&id, &text, Some(as_path.to_path_buf()))
            .map_err(|e| t!("cli.error.set_file", file = as_path.display(), error = e));
    }
    list.sets
        .iter()
        .filter(|s| s.matches(value))
        .max_by_key(|s| s.is_own())
        .cloned()
        .ok_or_else(|| t!("cli.error.set_unknown", name = value))
}

/// The paths of the `file` lines for the computer of `vars`; an unknown variable is an error.
pub fn paths(set: &LogSetDef, vars: &Vars) -> Result<Vec<PathBuf>, String> {
    set.files
        .iter()
        .map(|f| {
            logset::resolve(f, vars)
                .map(PathBuf::from)
                .map_err(|name| t!("logset.unknown_var", name = name))
        })
        .collect()
}

/// The text for a file that cannot be opened (the wording of the Open log set window).
fn problem(state: &FileState) -> String {
    match state {
        FileState::Found { .. } => String::new(),
        FileState::NotFound => t!("logset.not_found"),
        FileState::AccessDenied => t!("logset.access_denied"),
        FileState::NotAFile => t!("logset.not_a_file"),
        FileState::Unreachable => t!("logset.unreachable"),
        FileState::NoShare => t!("logset.no_share"),
        FileState::TimedOut => t!("logset.timed_out"),
        FileState::Error(e) => t!("logset.error", error = e),
    }
}

/// The logs of the set `value` for this computer or `computer`, all of which must exist.
/// `Err` = the message to show (unknown set, bad file, unknown variable, missing logs).
pub fn open(value: &str, computer: Option<&str>, rotated: bool) -> Result<Vec<PathBuf>, String> {
    let set = find(value, &logset::load_all())?;
    let vars = match computer {
        Some(c) => logset::remote_vars(
            &logset::computer_name(c).ok_or_else(|| t!("logset.remote.invalid", name = c))?,
        ),
        None => logset::this_computer_vars(),
    };
    let paths = paths(&set, &vars)?;
    let bad: Vec<String> = paths
        .iter()
        .filter_map(|p| match logset::check_file(p, rotated) {
            FileState::Found { .. } => None,
            s => Some(format!("{}: {}", p.display(), problem(&s))),
        })
        .collect();
    if bad.is_empty() {
        Ok(paths)
    } else {
        Err(format!(
            "{}\n\n{}",
            t!("cli.error.set_logs", name = set.id),
            bad.join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> SetList {
        let mk = |id: &str, text: &str, own: bool| {
            let mut s = LogSetDef::parse(id, text, None).unwrap();
            s.priority = if own {
                crate::logdef::PRIO_USER
            } else {
                crate::logdef::PRIO_BUILTIN
            };
            s
        };
        SetList {
            sets: vec![
                mk(
                    "a.set",
                    "name=SCCM client\nname.sk=Klient\nfile=%CCMLOGS%\\x.log\n",
                    false,
                ),
                mk("b.set", "name=SCCM client\nfile=C:\\own.log\n", true),
            ],
            errors: Vec::new(),
        }
    }

    #[test]
    fn finds_by_name_id_and_language() {
        let l = list();
        assert_eq!(find("a", &l).unwrap().id, "a.set");
        assert_eq!(find("A.SET", &l).unwrap().id, "a.set");
        assert_eq!(find(" klient ", &l).unwrap().id, "a.set");
        // The same name: the user's own set wins.
        assert_eq!(find("sccm client", &l).unwrap().id, "b.set");
        assert!(find("nothing", &l).is_err());
    }

    #[test]
    fn resolves_paths_for_a_computer() {
        let l = list();
        let set = find("b", &l).unwrap();
        let vars = logset::remote_vars("PC01");
        assert_eq!(
            paths(&set, &vars).unwrap(),
            vec![PathBuf::from(r"\\PC01\c$\own.log")]
        );
        let bad = LogSetDef::parse("c.set", "file=%NOPE%\\x.log", None).unwrap();
        assert!(paths(&bad, &vars).is_err());
    }
}
