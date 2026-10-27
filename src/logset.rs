//! Log sets (#27) – a named group of logs for one troubleshooting scenario, opened in one step
//! as one view merged by time.
//!
//! A log set is a UTF-8 text file named `<name>.set` with one `key = value` pair per line; `#`
//! starts a comment line. Keys:
//!
//! | key | meaning |
//! |---|---|
//! | `name` | name shown in the UI (English / default); missing = the file name without `.set` |
//! | `name.<language>` | name in a language, e.g. `name.sk`, `name.de`, `name.pt-br` |
//! | `description`, `description.<language>` | one or two sentences shown under the name |
//! | `file` | a log of the set; one line per log, in the order shown |
//!
//! Unknown keys and lines without `=` are ignored (a newer set file still loads in an older
//! version). Paths may use the variables `%CCMLOGS%` (ConfigMgr client logs), `%WINDIR%`,
//! `%PROGRAMDATA%` and `%SYSTEMDRIVE%` (case-insensitive); locally any other environment variable
//! works too. Sets are built in (`logsets/*.set` in the source tree) and loaded **without
//! recompiling** from `logsets\` next to the executable (a file with the same name replaces the
//! built-in one) and from `<config dir>/RusTrace/logsets/` (the user's own sets – they replace
//! nothing, both are offered), the same rules as for the log definitions (`logdef::load_from`).

use crate::logdef::{PRIO_BUILTIN, PRIO_PROGRAM, PRIO_USER};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Built-in sets (file name, content).
const BUILTIN: &[(&str, &str)] = &[
    ("appdeploy.set", include_str!("../logsets/appdeploy.set")),
    ("swupdates.set", include_str!("../logsets/swupdates.set")),
    (
        "clientinstall.set",
        include_str!("../logsets/clientinstall.set"),
    ),
    ("policy.set", include_str!("../logsets/policy.set")),
    ("intune.set", include_str!("../logsets/intune.set")),
];

/// A parsed log set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogSetDef {
    /// File name, e.g. `appdeploy.set`.
    pub id: String,
    /// Where it was loaded from (`None` = built-in).
    pub path: Option<PathBuf>,
    /// `PRIO_BUILTIN` / `PRIO_PROGRAM` (predefined) or `PRIO_USER` (own).
    pub priority: u8,
    /// Language code (lower-case, "" = default) → text.
    names: BTreeMap<String, String>,
    descriptions: BTreeMap<String, String>,
    /// The `file` lines as written (with variables).
    pub files: Vec<String>,
}

/// The text of a language: `code` itself, its base language ("de-AT" → "de"), then the default.
fn localized<'a>(map: &'a BTreeMap<String, String>, code: &str) -> Option<&'a str> {
    let code = code.to_ascii_lowercase().replace('_', "-");
    let base = code.split('-').next().unwrap_or("");
    map.get(&code)
        .or_else(|| map.get(base))
        .or_else(|| map.get(""))
        .map(String::as_str)
}

impl LogSetDef {
    /// Parses the text of a set file.
    pub fn parse(id: &str, text: &str, path: Option<PathBuf>) -> Result<LogSetDef, String> {
        let mut set = LogSetDef {
            id: id.to_owned(),
            path,
            priority: PRIO_BUILTIN,
            names: BTreeMap::new(),
            descriptions: BTreeMap::new(),
            files: Vec::new(),
        };
        for line in text.lines() {
            let line = line.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            if v.is_empty() {
                continue;
            }
            let (key, lang) = k.split_once('.').unwrap_or((&k, ""));
            match key {
                "name" => {
                    set.names.insert(lang.to_owned(), v.to_owned());
                }
                "description" => {
                    set.descriptions.insert(lang.to_owned(), v.to_owned());
                }
                "file" if lang.is_empty() => set.files.push(v.to_owned()),
                _ => {}
            }
        }
        if set.files.is_empty() {
            return Err("no \"file = …\" line".into());
        }
        Ok(set)
    }

    /// Name in the language `code` (falls back to the default name, then the file name).
    pub fn name(&self, code: &str) -> String {
        localized(&self.names, code).map_or_else(
            || self.id.strip_suffix(".set").unwrap_or(&self.id).to_owned(),
            str::to_owned,
        )
    }

    /// Description in the language `code` (falls back to the default one).
    pub fn description(&self, code: &str) -> Option<&str> {
        localized(&self.descriptions, code)
    }

    /// A set from the user's folder.
    pub fn is_own(&self) -> bool {
        self.priority == PRIO_USER
    }

    /// Identifies the set across reloads (remembered as the last chosen set).
    pub fn key(&self) -> String {
        let group = if self.is_own() { "own" } else { "predefined" };
        format!("{group}/{}", self.id.to_lowercase())
    }
}

// ----------------------------------------------------------------------
// Variables
// ----------------------------------------------------------------------

/// Values of the path variables of a computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vars {
    pub windir: String,
    pub programdata: String,
    pub ccmlogs: String,
    pub systemdrive: String,
    /// Other `%NAME%` variables come from the environment (only on this computer).
    pub env: bool,
}

/// Variables of this computer: the environment, `%CCMLOGS%` = the log folder of the ConfigMgr
/// client from the registry (`ccm_log_dir`), else `%WINDIR%\CCM\Logs`.
pub fn local_vars(ccm_log_dir: Option<String>) -> Vars {
    let env = |names: &[&str], default: &str| {
        names
            .iter()
            .find_map(|n| std::env::var(n).ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| default.to_owned())
    };
    let windir = env(&["WINDIR", "SystemRoot"], r"C:\Windows");
    let trim = |s: String| s.trim_end_matches(['\\', '/']).to_owned();
    Vars {
        ccmlogs: ccm_log_dir
            .map(trim)
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| format!(r"{windir}\CCM\Logs")),
        programdata: trim(env(&["PROGRAMDATA", "ALLUSERSPROFILE"], r"C:\ProgramData")),
        systemdrive: trim(env(&["SystemDrive"], "C:")),
        windir: trim(windir),
        env: true,
    }
}

/// Replaces the `%NAME%` variables of a `file` line; an unknown variable is an error (its name).
pub fn resolve(file: &str, vars: &Vars) -> Result<String, String> {
    let mut out = String::with_capacity(file.len() + 32);
    let mut rest = file;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let name = &after[..end];
        let value = match name.to_ascii_uppercase().as_str() {
            "CCMLOGS" => Some(vars.ccmlogs.clone()),
            "WINDIR" | "SYSTEMROOT" => Some(vars.windir.clone()),
            "PROGRAMDATA" => Some(vars.programdata.clone()),
            "SYSTEMDRIVE" => Some(vars.systemdrive.clone()),
            _ if vars.env && !name.is_empty() => std::env::var(name).ok(),
            _ => None,
        };
        match value {
            Some(v) => out.push_str(&v),
            None => return Err(format!("%{name}%")),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

// ----------------------------------------------------------------------
// Checking the files
// ----------------------------------------------------------------------

/// What a check of one file of a set found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileState {
    /// Size, last write time (local, ISO `YYYY-MM-DD HH:MM:SS`) and the number of rotated older
    /// parts (`name.lo_`, `name-YYYYMMDD-HHMMSS.log`) that are opened with it.
    Found {
        size: u64,
        modified: Option<String>,
        rotated: usize,
    },
    NotFound,
    AccessDenied,
    /// Not a file (a folder).
    NotAFile,
    Error(String),
}

impl FileState {
    pub fn exists(&self) -> bool {
        matches!(self, FileState::Found { .. })
    }
}

/// Checks one file (`rotated` = also count its rotated older parts). Can block for a long time
/// on a network path – call it in a background thread.
pub fn check_file(path: &Path, rotated: bool) -> FileState {
    match std::fs::metadata(path) {
        Ok(m) if !m.is_file() => FileState::NotAFile,
        Ok(m) => FileState::Found {
            size: m.len(),
            modified: m.modified().ok().map(|t| {
                chrono::DateTime::<chrono::Local>::from(t)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            }),
            rotated: if rotated {
                crate::parser::rotated_parts(path).len()
            } else {
                0
            },
        },
        Err(e) => io_state(&e),
    }
}

/// The state for an I/O error.
pub fn io_state(e: &std::io::Error) -> FileState {
    match e.kind() {
        std::io::ErrorKind::NotFound => FileState::NotFound,
        std::io::ErrorKind::PermissionDenied => FileState::AccessDenied,
        _ => FileState::Error(e.to_string()),
    }
}

// ----------------------------------------------------------------------
// Loading
// ----------------------------------------------------------------------

/// All loaded sets (predefined first, then own; each group by file name) and the errors of files
/// that could not be used.
#[derive(Default)]
pub struct SetList {
    pub sets: Vec<LogSetDef>,
    pub errors: Vec<String>,
}

/// `.set` files of a folder: (file name, text, path); unreadable files go to `errors`.
fn read_set_files(dir: &Path, errors: &mut Vec<String>) -> Vec<(String, String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension()
            .and_then(|x| x.to_str())
            .is_none_or(|x| !x.eq_ignore_ascii_case("set"))
        {
            continue;
        }
        let Some(id) = p.file_name().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        match std::fs::read_to_string(&p) {
            Ok(t) => out.push((id, t, p)),
            Err(err) => errors.push(format!("{}: {err}", p.display())),
        }
    }
    out
}

/// Loads the sets. `dirs` = [folder next to the program, user's folder] (see `set_dirs`):
/// predefined = built-in, a file with the same name next to the program replaces the built-in
/// one; own = the user's folder, replacing nothing.
pub fn load_from(dirs: &[PathBuf]) -> SetList {
    let mut predefined: BTreeMap<String, (String, String, Option<PathBuf>, u8)> = BUILTIN
        .iter()
        .map(|(id, t)| {
            (
                id.to_lowercase(),
                ((*id).to_owned(), (*t).to_owned(), None, PRIO_BUILTIN),
            )
        })
        .collect();
    let mut list = SetList::default();
    if let Some(dir) = dirs.first() {
        for (id, text, path) in read_set_files(dir, &mut list.errors) {
            predefined.insert(id.to_lowercase(), (id, text, Some(path), PRIO_PROGRAM));
        }
    }
    let mut own = BTreeMap::new();
    if let Some(dir) = dirs.get(1) {
        for (id, text, path) in read_set_files(dir, &mut list.errors) {
            own.insert(id.to_lowercase(), (id, text, Some(path), PRIO_USER));
        }
    }
    for (id, text, path, priority) in predefined.into_values().chain(own.into_values()) {
        let shown = path
            .as_ref()
            .map_or_else(|| id.clone(), |p| p.display().to_string());
        match LogSetDef::parse(&id, &text, path) {
            Ok(s) => list.sets.push(LogSetDef { priority, ..s }),
            Err(e) => list.errors.push(format!("{shown}: {e}")),
        }
    }
    list
}

/// Folders searched for set files: next to the program, then the user's folder.
pub fn set_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("logsets")))
    {
        dirs.push(d);
    }
    if let Some(d) = user_set_dir() {
        dirs.push(d);
    }
    dirs
}

/// Per-user sets folder (`%APPDATA%\RusTrace\logsets` on Windows).
pub fn user_set_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("RusTrace").join("logsets"))
}

pub fn load_all() -> SetList {
    load_from(&set_dirs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        Vars {
            windir: r"D:\Win".into(),
            programdata: r"D:\PD".into(),
            ccmlogs: r"E:\CCMLogs".into(),
            systemdrive: "D:".into(),
            env: false,
        }
    }

    #[test]
    fn parse_set_file() {
        let text = "\u{feff}# comment\nname = Deployment\nname.sk = Nasadenie\nNAME.DE = Bereitstellung\n\
                    description = Apps\nfile = %CCMLOGS%\\AppEnforce.log\n\nno equals sign here\n\
                    unknown.key = ignored\nfile = %WINDIR%\\ccmsetup\\Logs\\ccmsetup.log\nfile =\n";
        let s = LogSetDef::parse("dep.set", text, None).unwrap();
        assert_eq!(
            s.files,
            [
                r"%CCMLOGS%\AppEnforce.log",
                r"%WINDIR%\ccmsetup\Logs\ccmsetup.log"
            ]
        );
        assert_eq!(s.name("en"), "Deployment");
        assert_eq!(s.name("sk"), "Nasadenie");
        assert_eq!(s.name("de-AT"), "Bereitstellung");
        assert_eq!(s.name("cs"), "Deployment");
        assert_eq!(s.description("sk"), Some("Apps"));
        assert_eq!(s.key(), "predefined/dep.set");

        let unnamed = LogSetDef::parse("My Set.set", "file = C:\\a.log", None).unwrap();
        assert_eq!(unnamed.name("en"), "My Set");
        assert_eq!(unnamed.description("en"), None);
        assert!(LogSetDef::parse("x.set", "name = Empty\n# file = a.log", None).is_err());
    }

    #[test]
    fn builtin_sets_parse_and_are_translated() {
        for (id, text) in BUILTIN {
            let s = LogSetDef::parse(id, text, None).unwrap_or_else(|e| panic!("{id}: {e}"));
            for code in ["", "sk", "cs", "de"] {
                assert!(s.names.contains_key(code), "{id}: name.{code}");
                assert!(
                    s.descriptions.contains_key(code),
                    "{id}: description.{code}"
                );
            }
            for f in &s.files {
                assert!(resolve(f, &vars()).is_ok(), "{id}: {f}");
                assert!(f.starts_with('%'), "{id}: {f} should start with a variable");
            }
        }
    }

    #[test]
    fn variables() {
        let v = vars();
        assert_eq!(
            resolve(r"%CCMLOGS%\AppEnforce.log", &v).unwrap(),
            r"E:\CCMLogs\AppEnforce.log"
        );
        assert_eq!(
            resolve(r"%windir%\Logs\CBS\CBS.log", &v).unwrap(),
            r"D:\Win\Logs\CBS\CBS.log"
        );
        assert_eq!(
            resolve(r"%ProgramData%\x\%SystemDrive%", &v).unwrap(),
            r"D:\PD\x\D:"
        );
        assert_eq!(resolve(r"C:\plain\a.log", &v).unwrap(), r"C:\plain\a.log");
        assert_eq!(resolve("50% done.log", &v).unwrap(), "50% done.log");
        assert_eq!(resolve(r"%TEMP%\a.log", &v).unwrap_err(), "%TEMP%");

        // Locally the environment fills other variables; the ConfigMgr folder from the registry.
        let local = local_vars(Some(r"F:\Logs\CCM\".into()));
        assert_eq!(local.ccmlogs, r"F:\Logs\CCM");
        assert!(!local.windir.ends_with('\\'));
        let default = local_vars(None);
        assert_eq!(default.ccmlogs, format!(r"{}\CCM\Logs", default.windir));
        // SAFETY: only this test reads this variable.
        unsafe { std::env::set_var("RUSTRACE_TEST_LOGSET_VAR", "X") };
        assert_eq!(
            resolve("%RUSTRACE_TEST_LOGSET_VAR%/a", &local).unwrap(),
            "X/a"
        );
    }

    #[test]
    fn loading_and_priorities() {
        let base = std::env::temp_dir().join(format!("rustrace_logsets_{}", std::process::id()));
        let (prog, user) = (base.join("prog"), base.join("user"));
        std::fs::create_dir_all(&prog).unwrap();
        std::fs::create_dir_all(&user).unwrap();
        std::fs::write(prog.join("policy.set"), "name = Policy 2\nfile = a.log").unwrap();
        std::fs::write(user.join("POLICY.set"), "name = My policy\nfile = b.log").unwrap();
        std::fs::write(user.join("broken.set"), "name = Broken").unwrap();
        std::fs::write(user.join("notes.txt"), "file = c.log").unwrap();
        let list = load_from(&[prog, user.clone()]);
        assert_eq!(list.sets.len(), BUILTIN.len() + 1);
        assert_eq!(list.errors.len(), 1, "{:?}", list.errors);
        let policy: Vec<_> = list
            .sets
            .iter()
            .filter(|s| s.id.eq_ignore_ascii_case("policy.set"))
            .collect();
        assert_eq!(policy.len(), 2);
        assert_eq!(policy[0].name("en"), "Policy 2");
        assert_eq!(policy[0].priority, PRIO_PROGRAM);
        assert_eq!(policy[1].name("en"), "My policy");
        assert!(policy[1].is_own());
        assert_eq!(policy[1].key(), "own/policy.set");
        assert_eq!(
            policy[1].path.as_deref(),
            Some(user.join("POLICY.set").as_path())
        );

        // File checks.
        let log = base.join("x.log");
        std::fs::write(&log, "12345").unwrap();
        std::fs::write(base.join("x.lo_"), "0").unwrap();
        match check_file(&log, true) {
            FileState::Found {
                size,
                modified,
                rotated,
            } => {
                assert_eq!((size, rotated), (5, 1));
                assert_eq!(modified.unwrap().len(), 19);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            check_file(&log, false),
            FileState::Found { rotated: 0, .. }
        ));
        assert_eq!(
            check_file(&base.join("none.log"), true),
            FileState::NotFound
        );
        assert_eq!(check_file(&base, true), FileState::NotAFile);
        let _ = std::fs::remove_dir_all(&base);
    }
}
