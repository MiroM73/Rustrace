//! Writing log sets (#76): *File → Save as log set* stores the opened logs as a `.set` file in
//! the user's folder.
//!
//! [`set_lines`] turns the paths of the opened logs back into `file` lines with variables (the
//! reverse of `logset::resolve`), [`update_text`] edits the text of a set file and keeps
//! everything it does not change, [`save`] writes the file. All but `save` are pure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::logset::{self, LogSetDef, Vars};

/// Longest file name (without `.set`) made from the name of a set, in characters.
const MAX_STEM: usize = 80;

// ----------------------------------------------------------------------
// Paths → `file` lines
// ----------------------------------------------------------------------

/// `path` without the folder `prefix`: the rest, empty or starting with a separator. The case
/// and the kind of the separators do not matter; `C:\Windows2\x` is not under `C:\Windows`.
fn strip_folder<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let prefix = prefix.trim_end_matches(['\\', '/']);
    if prefix.is_empty() {
        return None;
    }
    let fold = |c: char| match c {
        '/' => '\\',
        c => c.to_lowercase().next().unwrap_or(c),
    };
    let mut rest = path.chars();
    for p in prefix.chars() {
        if fold(rest.next()?) != fold(p) {
            return None;
        }
    }
    let rest = rest.as_str();
    (rest.is_empty() || rest.starts_with(['\\', '/'])).then_some(rest)
}

/// `path` with the longest of the `folders` (folder, variable) it is in replaced by `%variable%`.
fn with_variable(path: &str, folders: &[(&str, &str)]) -> Option<String> {
    folders
        .iter()
        .filter_map(|(folder, name)| strip_folder(path, folder).map(|rest| (rest, *name)))
        // The shortest rest = the longest folder.
        .min_by_key(|(rest, _)| rest.len())
        .map(|(rest, name)| format!("%{name}%{rest}"))
}

/// The computer and drive (`None` = `admin$`) of a path on an administrative share, with the
/// rest of the path.
fn admin_share(path: &str) -> Option<(String, Option<char>, String)> {
    let root = logset::share_root(path)?;
    let host = logset::share_host(&root).to_owned();
    let share = root.rsplit('\\').next()?;
    let drive = match share.as_bytes() {
        [d, b'$'] if d.is_ascii_alphabetic() => Some((*d as char).to_ascii_uppercase()),
        _ if share.eq_ignore_ascii_case("admin$") => None,
        _ => return None,
    };
    let rest = path[2..]
        .split(['\\', '/'])
        .filter(|p| !p.is_empty())
        .skip(2)
        .collect::<Vec<_>>()
        .join("\\");
    Some((host, drive, rest))
}

/// A path on this computer with `%CCMLOGS%`, `%WINDIR%` or `%PROGRAMDATA%`, else as it is.
fn local_line(path: &str, vars: &Vars) -> String {
    with_variable(
        path,
        &[
            (&vars.ccmlogs, "CCMLOGS"),
            (&vars.windir, "WINDIR"),
            (&vars.programdata, "PROGRAMDATA"),
        ],
    )
    .unwrap_or_else(|| path.to_owned())
}

/// A path on an administrative share of a remote computer as the path that
/// `logset::resolve` turns back into it: with a variable, or with a drive letter.
fn remote_line(path: &str, host: &str, drive: Option<char>, rest: &str) -> String {
    let vars = logset::remote_vars(host);
    let windows = format!(r"{}\Windows", vars.systemdrive);
    let windows_logs = format!(r"{windows}\CCM\Logs");
    let full = match drive {
        Some(d) => format!(r"\\{host}\{}$\{rest}", d.to_ascii_lowercase()),
        None => format!(r"\\{host}\admin$\{rest}"),
    };
    with_variable(
        &full,
        &[
            (&vars.ccmlogs, "CCMLOGS"),
            (&vars.windir, "WINDIR"),
            (&windows_logs, "CCMLOGS"),
            (&windows, "WINDIR"),
            (&vars.programdata, "PROGRAMDATA"),
        ],
    )
    .unwrap_or_else(|| match drive {
        Some(d) => format!(r"{d}:\{rest}"),
        None => path.to_owned(),
    })
}

/// The `file` lines of a set made of the logs `paths`, so that the set works on any computer:
/// known folders become variables, and the paths on the administrative shares of **one** remote
/// computer become paths of "the computer" (`\\pc01\d$\Logs\x.log` → `D:\Logs\x.log`). When the
/// logs come from this and a remote computer, or from several remote ones, the remote paths stay
/// as they are. `local` = the variables of this computer. No line is repeated.
pub fn set_lines(paths: &[String], local: &Vars) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    let mut here = false;
    for p in paths {
        match admin_share(p) {
            Some((host, ..)) => {
                let host = host.to_lowercase();
                if !hosts.contains(&host) {
                    hosts.push(host);
                }
            }
            None => here |= logset::share_root(p).is_none(),
        }
    }
    let one_remote = !here && hosts.len() == 1;
    let mut lines: Vec<String> = Vec::new();
    for p in paths {
        let line = match admin_share(p) {
            Some((host, drive, rest)) if one_remote => remote_line(p, &host, drive, &rest),
            Some(_) => p.clone(),
            None => local_line(p, local),
        };
        if !lines.iter().any(|l| l.eq_ignore_ascii_case(&line)) {
            lines.push(line);
        }
    }
    lines
}

// ----------------------------------------------------------------------
// File name, collisions
// ----------------------------------------------------------------------

/// The file name (`….set`) for a set called `name`.
pub fn file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(MAX_STEM)
        .collect();
    let mut stem = cleaned.trim_matches([' ', '.']).to_owned();
    if stem.is_empty() {
        stem = "logset".into();
    }
    // Device names of Windows cannot be file names, with any extension.
    let first = stem
        .split('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    let numbered = |p: &str| {
        first
            .strip_prefix(p)
            .is_some_and(|n| n.len() == 1 && n.chars().all(|c| c.is_ascii_digit()))
    };
    if matches!(first.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered("COM") || numbered("LPT")
    {
        stem.insert(0, '_');
    }
    format!("{stem}.set")
}

/// What saving under a name would replace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collision {
    /// The file that would be overwritten.
    pub path: PathBuf,
    /// An own set of this name exists: its file is updated (guides, comments and translations
    /// stay). Otherwise only the file name is taken and the file is replaced.
    pub same_name: bool,
}

/// Whether a set called `name` collides with an own set of that name, or with a file of the
/// user's folder `dir` (`files` = the file names in it). Predefined sets do not count.
pub fn collision(
    sets: &[LogSetDef],
    dir: &Path,
    files: &[String],
    name: &str,
) -> Option<Collision> {
    let own = sets.iter().filter(|s| s.is_own() && s.has_name(name));
    if let Some(path) = own.filter_map(|s| s.path.clone()).next() {
        return Some(Collision {
            path,
            same_name: true,
        });
    }
    let wanted = file_name(name).to_lowercase();
    files
        .iter()
        .find(|f| f.to_lowercase() == wanted)
        .map(|f| Collision {
            path: dir.join(f),
            same_name: false,
        })
}

/// The names of the files in the user's folder.
pub fn files_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

// ----------------------------------------------------------------------
// The text of a set file
// ----------------------------------------------------------------------

/// One value of a set file: line breaks and runs of white space become one space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What [`update_text`] changes; `None` = keep it as it is.
#[derive(Default)]
pub struct Changes<'a> {
    /// The name shown in the language of the user interface (empty = keep).
    pub name: Option<&'a str>,
    /// The description shown in that language; empty = remove the description (all languages).
    pub description: Option<&'a str>,
    /// All `file` lines.
    pub files: Option<&'a [String]>,
}

/// The index of the `key` / `lang` line, where one is.
fn position(lines: &[String], key: &str) -> Option<usize> {
    lines
        .iter()
        .position(|l| logset::split_line(l).is_some_and(|(k, _, _)| k == key))
}

/// Sets `key` in the language the user interface (`code`) shows: that line is rewritten, so the
/// change is visible even in a set with translations; a set without such a text gets the plain
/// `key`, inserted at `at`.
fn set_localized(lines: &mut Vec<String>, key: &str, code: &str, value: &str, at: usize) {
    let present: BTreeMap<String, String> = lines
        .iter()
        .filter_map(|l| logset::split_line(l))
        .filter(|(k, _, v)| k == key && !v.is_empty())
        .map(|(_, lang, v)| (lang, v.to_owned()))
        .collect();
    let lang = logset::localized_entry(&present, code).map_or("", |(lang, _)| lang);
    let line = if lang.is_empty() {
        format!("{key} = {value}")
    } else {
        format!("{key}.{lang} = {value}")
    };
    let same =
        |l: &String| logset::split_line(l).is_some_and(|(k, other, _)| k == key && other == lang);
    match lines.iter().position(same) {
        Some(first) => {
            // A repeated key: the last one would win, so only one stays.
            let mut i = 0;
            lines.retain(|l| {
                i += 1;
                i - 1 <= first || !same(l)
            });
            lines[first] = line;
        }
        None => lines.insert(at.min(lines.len()), line),
    }
}

/// The text of a set file with `changes`; comments, `guide` lines, unknown keys and the order of
/// the lines stay. `code` = the language of the user interface. An empty `text` makes a new
/// file. Line endings are kept.
pub fn update_text(text: &str, code: &str, changes: &Changes) -> String {
    let text = text.trim_start_matches('\u{feff}');
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    // New keys go after the leading comments.
    let top = |lines: &[String]| {
        lines
            .iter()
            .position(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
            .unwrap_or(lines.len())
    };
    if let Some(name) = changes.name.map(one_line).filter(|n| !n.is_empty()) {
        let at = top(&lines);
        set_localized(&mut lines, "name", code, &name, at);
    }
    match changes.description.map(one_line) {
        Some(d) if d.is_empty() => {
            lines.retain(|l| logset::split_line(l).is_none_or(|(k, _, _)| k != "description"));
        }
        Some(d) => {
            // After the (last) name line.
            let at = lines
                .iter()
                .rposition(|l| logset::split_line(l).is_some_and(|(k, _, _)| k == "name"))
                .map_or_else(|| top(&lines), |i| i + 1);
            set_localized(&mut lines, "description", code, &d, at);
        }
        None => {}
    }
    if let Some(files) = changes.files {
        let at = position(&lines, "file");
        lines.retain(|l| {
            logset::split_line(l).is_none_or(|(k, lang, _)| k != "file" || !lang.is_empty())
        });
        let at = at.unwrap_or(lines.len()).min(lines.len());
        lines.splice(at..at, files.iter().map(|f| format!("file = {f}")));
    }
    let mut out = lines.join(eol);
    out.push_str(eol);
    out
}

/// Writes `changes` to the set file `path`: with `keep` the existing file is updated, otherwise
/// a new text replaces it. The file is written completely or not at all.
pub fn save(path: &Path, keep: bool, code: &str, changes: &Changes) -> std::io::Result<()> {
    let old = if keep {
        std::fs::read_to_string(path)?
    } else {
        "# RusTrace log set\n".to_owned()
    };
    let text = update_text(&old, code, changes);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("set.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logdef::PRIO_USER;

    fn local() -> Vars {
        Vars {
            windir: r"C:\Windows".into(),
            programdata: r"C:\ProgramData".into(),
            // A client with its logs moved (registry).
            ccmlogs: r"C:\Windows\Custom\CCMLogs".into(),
            systemdrive: "C:".into(),
            env: false,
            remote: None,
        }
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn local_paths_get_variables() {
        let paths = strings(&[
            r"c:\windows\custom\ccmlogs\AppEnforce.log",
            r"C:\Windows\Logs\CBS\CBS.log",
            "C:/ProgramData/App/a.log",
            r"C:\Windows2\x.log",
            r"C:\MyApp\x.log",
            r"\\server\logs\y.log",
        ]);
        let lines = set_lines(&paths, &local());
        assert_eq!(
            lines,
            [
                r"%CCMLOGS%\AppEnforce.log",
                r"%WINDIR%\Logs\CBS\CBS.log",
                "%PROGRAMDATA%/App/a.log",
                r"C:\Windows2\x.log",
                r"C:\MyApp\x.log",
                r"\\server\logs\y.log",
            ]
        );
        // Resolving gives the paths again (the case of a variable's folder is its own).
        for (line, path) in lines.iter().zip(&paths) {
            let back = logset::resolve(line, &local()).unwrap();
            assert!(
                back.eq_ignore_ascii_case(path) || path.contains('/'),
                "{back}"
            );
        }
    }

    #[test]
    fn paths_of_one_remote_computer() {
        let paths = strings(&[
            r"\\PC01\admin$\CCM\Logs\AppEnforce.log",
            r"\\pc01\ADMIN$\Logs\CBS\CBS.log",
            r"\\pc01\c$\ProgramData\App\a.log",
            r"\\pc01\d$\Logs\b.log",
            r"\\pc01\c$\Windows\ccmsetup\Logs\ccmsetup.log",
            // The same log reached another way: one line.
            r"\\pc01\c$\Windows\CCM\Logs\AppEnforce.log",
            r"\\server\share\y.log",
        ]);
        let lines = set_lines(&paths, &local());
        assert_eq!(
            lines,
            [
                r"%CCMLOGS%\AppEnforce.log",
                r"%WINDIR%\Logs\CBS\CBS.log",
                r"%PROGRAMDATA%\App\a.log",
                r"D:\Logs\b.log",
                r"%WINDIR%\ccmsetup\Logs\ccmsetup.log",
                r"\\server\share\y.log",
            ]
        );
        let remote = logset::remote_vars("pc02");
        assert_eq!(
            logset::resolve(&lines[3], &remote).unwrap(),
            r"\\pc02\d$\Logs\b.log"
        );
        let same = logset::remote_vars("PC01");
        for (line, path) in lines.iter().zip(&paths).take(4) {
            let back = logset::resolve(line, &same).unwrap();
            assert!(back.eq_ignore_ascii_case(path), "{back} / {path}");
        }
    }

    #[test]
    fn mixed_computers_stay_literal() {
        let two = strings(&[r"\\pc01\c$\a.log", r"\\pc02\c$\a.log"]);
        assert_eq!(set_lines(&two, &local()), two);
        let mixed = strings(&[r"\\pc01\admin$\CCM\Logs\a.log", r"C:\Windows\b.log"]);
        assert_eq!(
            set_lines(&mixed, &local()),
            [r"\\pc01\admin$\CCM\Logs\a.log", r"%WINDIR%\b.log"]
        );
    }

    #[test]
    fn file_names() {
        assert_eq!(file_name("My app"), "My app.set");
        assert_eq!(
            file_name(r#" a/b\c:d*e?f"g<h>i|j. "#),
            "a_b_c_d_e_f_g_h_i_j.set"
        );
        assert_eq!(file_name(" .. "), "logset.set");
        assert_eq!(file_name("con"), "_con.set");
        assert_eq!(file_name("LPT1.x"), "_LPT1.x.set");
        assert_eq!(file_name("Console"), "Console.set");
        assert_eq!(file_name(&"č".repeat(200)).chars().count(), MAX_STEM + 4);
    }

    #[test]
    fn collisions() {
        let dir = Path::new("sets");
        let own = |id: &str, text: &str| {
            let mut set = LogSetDef::parse(id, text, Some(dir.join(id))).unwrap();
            set.priority = PRIO_USER;
            set
        };
        let sets = [
            LogSetDef::parse("policy.set", "name = Policy\nfile = a", None).unwrap(),
            own("old.set", "name = My app\nname.sk = Moja appka\nfile = a"),
        ];
        let files = strings(&["old.set", "A_B.set"]);
        let hit = |name: &str| collision(&sets, dir, &files, name);
        let same = |id: &str| {
            Some(Collision {
                path: dir.join(id),
                same_name: true,
            })
        };
        assert_eq!(hit("my APP"), same("old.set"));
        assert_eq!(hit(" moja appka "), same("old.set"));
        // Only the file name is taken.
        assert_eq!(
            hit("a?b"),
            Some(Collision {
                path: dir.join("A_B.set"),
                same_name: false,
            })
        );
        // A predefined set may have the name; the file name of a set is not its name.
        assert_eq!(hit("Policy"), None);
        assert_eq!(hit("New"), None);
    }

    #[test]
    fn new_set_text() {
        let files = strings(&[r"%WINDIR%\a.log", r"D:\b=c.log"]);
        let text = update_text(
            "# RusTrace log set\n",
            "sk",
            &Changes {
                name: Some(" Moja  sada "),
                description: Some("Prvý riadok\r\ndruhý = riadok\n"),
                files: Some(&files),
            },
        );
        assert_eq!(
            text,
            "# RusTrace log set\nname = Moja sada\ndescription = Prvý riadok druhý = riadok\n\
             file = %WINDIR%\\a.log\nfile = D:\\b=c.log\n"
        );
        let s = LogSetDef::parse("m.set", &text, None).unwrap();
        assert_eq!(s.name("sk"), "Moja sada");
        assert_eq!(s.description("sk"), Some("Prvý riadok druhý = riadok"));
        assert_eq!(s.files, files);
    }

    #[test]
    fn editing_keeps_the_rest() {
        let old = "\u{feff}# mine\r\nname = App\r\nname.sk = Appka\r\ndescription.de = Alt\r\n\
                   # logs\r\nfile = a.log\r\nguide = g.md\r\nfile = b.log\r\nfuture = x\r\n";
        // The Slovak name is the one shown, so it changes; no description is shown, so the new
        // one is the plain one.
        let text = update_text(
            old,
            "sk-SK",
            &Changes {
                name: Some("Nová"),
                description: Some("Popis"),
                files: None,
            },
        );
        assert_eq!(
            text,
            "# mine\r\nname = App\r\nname.sk = Nová\r\ndescription = Popis\r\n\
             description.de = Alt\r\n# logs\r\nfile = a.log\r\nguide = g.md\r\nfile = b.log\r\n\
             future = x\r\n"
        );
        // An empty description removes it in every language; new logs take the old place.
        let files = strings(&["c.log"]);
        let text = update_text(
            &text,
            "en",
            &Changes {
                name: None,
                description: Some("  "),
                files: Some(&files),
            },
        );
        assert_eq!(
            text,
            "# mine\r\nname = App\r\nname.sk = Nová\r\n# logs\r\nfile = c.log\r\n\
             guide = g.md\r\nfuture = x\r\n"
        );
        // A repeated key would hide the change.
        let text = update_text(
            "name = A\nname = B\nfile = a\n",
            "en",
            &Changes {
                name: Some("C"),
                ..Changes::default()
            },
        );
        assert_eq!(text, "name = C\nfile = a\n");
    }

    #[test]
    fn saving_a_file() {
        let dir = std::env::temp_dir().join(format!("rustrace_setsave_{}", std::process::id()));
        let path = dir.join("new").join("x.set");
        let files = strings(&["a.log"]);
        let changes = Changes {
            name: Some("X"),
            description: None,
            files: Some(&files),
        };
        // Creates the folder; replaces an existing file unless it is to be kept.
        save(&path, false, "en", &changes).unwrap();
        std::fs::write(&path, "# note\nname = X\nguide = g\nfile = old\n").unwrap();
        save(&path, true, "en", &changes).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# note\nname = X\nguide = g\nfile = a.log\n"
        );
        save(&path, false, "en", &changes).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# RusTrace log set\nname = X\nfile = a.log\n"
        );
        assert_eq!(files_in(path.parent().unwrap()), ["x.set"]);
        assert!(save(&dir.join("none.set"), true, "en", &changes).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
