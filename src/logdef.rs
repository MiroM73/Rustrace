//! Log definitions – describe the layout of other log formats (IIS, DISM, CBS …) in text files.
//!
//! A definition is a UTF-8 text file named `<name>.def` (e.g. `cbs.log.def`) with one
//! `key = value` pair per line; `#` starts a comment line. Keys:
//!
//! | key | meaning |
//! |---|---|
//! | `name` | name shown in the UI |
//! | `files` | file name patterns (`*`, `?`, case-insensitive), separated by `;` – default: the file name without `.def` |
//! | `detect` | regex; a file whose name matches no definition uses this one when one of its first lines matches |
//! | `format` | `regex` (default) or `w3c` (column names from the `#Fields:` header, space-separated values, `-` = empty) |
//! | `pattern` | regex for one entry (format `regex`); every **named group** is a column |
//! | `fields` | w3c: column names used until a `#Fields:` header is found |
//! | `skip` | regex; matching lines are ignored |
//! | `unmatched` | lines not matching `pattern`: `append` to the previous entry (default for `regex`), `entry` = own entry (default for `w3c`), `skip` |
//! | `role.message`, `role.component`, `role.thread`, `role.datetime` | columns (one or more, joined by a space) that fill the standard columns; default: the column with that name (`datetime` also `date` + `time`) |
//! | `role.severity` | column whose value gives the severity; default: `severity` |
//! | `severity.error`, `severity.warning` | regexes (case-insensitive) for the severity value |
//! | `datetime.format` | chrono format(s) of the date/time, separated by `\|`, e.g. `%Y-%m-%d %H:%M:%S` |
//! | `timezone` | `local` (default) or `utc` (converted to local time) |
//! | `columns` | order of the columns in the table (role names or column names); default: order in the pattern / header |
//! | `title.<column>` | column header (`title.message = Request`) |
//!
//! Columns that are not used by `role.message/component/thread/datetime` are kept as extra
//! columns (shown in the table, searchable). Definitions are built in (`logdef/*.def` in the
//! source tree) and loaded **without recompiling** from `logdef\` next to the executable and
//! `<config dir>/RusTrace/logdef/`; a file with the same name overrides the built-in one.

use crate::parser::{keyword_sev, LogEntry, Severity};
use regex::{Regex, RegexBuilder};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Built-in definitions (file name, content).
const BUILTIN: &[(&str, &str)] = &[
    ("cbs.log.def", include_str!("../logdef/cbs.log.def")),
    ("dism.log.def", include_str!("../logdef/dism.log.def")),
    ("iisweb.log.def", include_str!("../logdef/iisweb.log.def")),
];

/// Date/time formats tried when `datetime.format` does not match.
const DEFAULT_DT_FORMATS: &[&str] = &[
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y/%m/%d %H:%M:%S%.f",
];

/// Standard columns every log has (filled by the CMTrace parser or by a definition's roles).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Role {
    Message,
    Component,
    DateTime,
    Thread,
}

impl Role {
    const ALL: [Role; 4] = [Role::Message, Role::Component, Role::DateTime, Role::Thread];

    fn key(self) -> &'static str {
        match self {
            Role::Message => "message",
            Role::Component => "component",
            Role::DateTime => "datetime",
            Role::Thread => "thread",
        }
    }

    fn from_key(s: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.key().eq_ignore_ascii_case(s))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ColumnKind {
    Role(Role),
    /// Index into `LogEntry::extra` / `Schema::extra_names`.
    Extra(usize),
}

/// A table column (after the line number).
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Column {
    pub kind: ColumnKind,
    /// Header text; `None` = the translated standard header.
    pub title: Option<String>,
}

/// The columns of the open log.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Schema {
    /// Name of the log definition in use; empty = built-in CMTrace / SMS / plain text parser.
    pub name: String,
    pub columns: Vec<Column>,
    /// Names of the extra columns (`LogEntry::extra` has the values in this order).
    pub extra_names: Vec<String>,
}

impl Default for Schema {
    fn default() -> Self {
        let col = |r| Column {
            kind: ColumnKind::Role(r),
            title: None,
        };
        Self {
            name: String::new(),
            columns: vec![
                col(Role::Message),
                col(Role::Component),
                col(Role::DateTime),
                col(Role::Thread),
            ],
            extra_names: Vec::new(),
        }
    }
}

impl Schema {
    fn extra_index(&mut self, name: &str) -> usize {
        match self.extra_names.iter().position(|n| n.eq_ignore_ascii_case(name)) {
            Some(i) => i,
            None => {
                self.extra_names.push(name.to_owned());
                self.extra_names.len() - 1
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unmatched {
    Append,
    Entry,
    Skip,
}

#[derive(Debug)]
enum Kind {
    Regex(Regex),
    W3c,
}

/// A parsed log definition.
#[derive(Debug)]
pub struct LogDef {
    /// File name, e.g. `cbs.log.def` – identifies the definition.
    pub id: String,
    pub name: String,
    /// Where it was loaded from (`None` = built-in).
    pub path: Option<PathBuf>,
    files: Vec<String>,
    detect: Option<Regex>,
    kind: Kind,
    skip: Option<Regex>,
    unmatched: Unmatched,
    /// Column names for each `Role` (indexed by `Role as usize`); empty = default.
    roles: [Vec<String>; 4],
    severity_field: Option<String>,
    sev_error: Regex,
    sev_warning: Regex,
    dt_formats: Vec<String>,
    utc: bool,
    columns: Option<Vec<String>>,
    /// Lower-case column name → header.
    titles: HashMap<String, String>,
    default_fields: Vec<String>,
}

fn build_regex(key: &str, pattern: &str, case_insensitive: bool) -> Result<Regex, String> {
    RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .size_limit(20 << 20)
        .build()
        .map_err(|e| format!("{key}: {e}"))
}

/// Splits a list value ("a b, c; d") into its items.
fn list(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|x| !x.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Simple wildcard match (`*` = any text, `?` = one character); both sides lower-case.
fn wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

impl LogDef {
    /// Parses the text of a definition file.
    pub fn parse(id: &str, text: &str, path: Option<PathBuf>) -> Result<LogDef, String> {
        let mut map: HashMap<String, String> = HashMap::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                return Err(format!("line {}: expected \"key = value\"", n + 1));
            };
            map.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
        }

        let mut roles: [Vec<String>; 4] = Default::default();
        let mut titles = HashMap::new();
        for (k, v) in &map {
            if let Some(name) = k.strip_prefix("title.") {
                titles.insert(name.to_owned(), v.clone());
            } else if let Some(role) = k.strip_prefix("role.") {
                if role == "severity" {
                    continue;
                }
                let r = Role::from_key(role).ok_or_else(|| format!("unknown role \"{role}\""))?;
                roles[r as usize] = list(v);
            } else if !matches!(
                k.as_str(),
                "name"
                    | "files"
                    | "detect"
                    | "format"
                    | "pattern"
                    | "fields"
                    | "skip"
                    | "unmatched"
                    | "severity.error"
                    | "severity.warning"
                    | "datetime.format"
                    | "timezone"
                    | "columns"
            ) {
                return Err(format!("unknown key \"{k}\""));
            }
        }

        let get = |k: &str| map.get(k).map(String::as_str).filter(|v| !v.is_empty());
        let kind = match get("format").unwrap_or("regex").to_ascii_lowercase().as_str() {
            "w3c" => Kind::W3c,
            "regex" => {
                let p = get("pattern").ok_or("missing key \"pattern\"")?;
                let re = build_regex("pattern", p, false)?;
                if re.capture_names().flatten().next().is_none() {
                    return Err("pattern: no named group (?P<name>…)".into());
                }
                Kind::Regex(re)
            }
            other => return Err(format!("format: unknown value \"{other}\" (regex, w3c)")),
        };
        // w3c lines never continue on the next line, so entries need not wait for the next one.
        let default_unmatched = if matches!(kind, Kind::W3c) { "entry" } else { "append" };
        let unmatched = match get("unmatched").unwrap_or(default_unmatched).to_ascii_lowercase().as_str() {
            "append" => Unmatched::Append,
            "entry" => Unmatched::Entry,
            "skip" => Unmatched::Skip,
            other => return Err(format!("unmatched: unknown value \"{other}\" (append, entry, skip)")),
        };
        let utc = match get("timezone").unwrap_or("local").to_ascii_lowercase().as_str() {
            "local" => false,
            "utc" => true,
            other => return Err(format!("timezone: unknown value \"{other}\" (local, utc)")),
        };
        let optional = |k: &str, ci: bool| get(k).map(|p| build_regex(k, p, ci)).transpose();
        let stem = id.strip_suffix(".def").unwrap_or(id);
        Ok(LogDef {
            id: id.to_owned(),
            name: get("name").unwrap_or(stem).to_owned(),
            path,
            files: get("files")
                .map(|f| {
                    f.split(';')
                        .map(|p| p.trim().to_lowercase())
                        .filter(|p| !p.is_empty())
                        .collect()
                })
                .unwrap_or_else(|| vec![stem.to_lowercase()]),
            detect: optional("detect", false)?,
            kind,
            skip: optional("skip", false)?,
            unmatched,
            roles,
            severity_field: get("role.severity").map(str::to_owned),
            sev_error: build_regex(
                "severity.error",
                get("severity.error").unwrap_or("^(e|err|error|fatal|critical|crit|3)$"),
                true,
            )?,
            sev_warning: build_regex(
                "severity.warning",
                get("severity.warning").unwrap_or("^(w|warn|warning|2)$"),
                true,
            )?,
            dt_formats: get("datetime.format")
                .map(|f| f.split('|').map(|x| x.trim().to_owned()).filter(|x| !x.is_empty()).collect())
                .unwrap_or_default(),
            utc,
            columns: get("columns").map(list),
            titles,
            default_fields: get("fields").map(list).unwrap_or_default(),
        })
    }

    /// Does the definition apply to this file name (by its `files` patterns)?
    pub fn matches_file(&self, file_name: &str) -> bool {
        let f = file_name.to_lowercase();
        self.files.iter().any(|p| wildcard(p, &f))
    }

    /// Does one of the given first lines of a file match `detect`?
    pub fn detects(&self, lines: &[&str]) -> bool {
        self.detect
            .as_ref()
            .is_some_and(|re| lines.iter().any(|l| re.is_match(l)))
    }

    /// Indexes of the fields that fill a role.
    fn role_fields(&self, role: Role, names: &[String]) -> Vec<usize> {
        let find = |n: &str| names.iter().position(|f| f.eq_ignore_ascii_case(n));
        let wanted = &self.roles[role as usize];
        if !wanted.is_empty() {
            return wanted.iter().filter_map(|n| find(n)).collect();
        }
        if let Some(i) = find(role.key()) {
            return vec![i];
        }
        if role == Role::DateTime
            && let (Some(d), Some(t)) = (find("date"), find("time"))
        {
            return vec![d, t];
        }
        Vec::new()
    }

    /// Maps field names (regex groups or w3c header) to roles and extra columns and sets
    /// the table columns of `schema`.
    fn mapping(&self, names: &[String], schema: &mut Schema) -> Mapping {
        let roles = Role::ALL.map(|r| self.role_fields(r, names));
        let role_of = |i: usize| Role::ALL.into_iter().find(|r| roles[*r as usize].contains(&i));
        let severity = names
            .iter()
            .position(|f| f.eq_ignore_ascii_case(self.severity_field.as_deref().unwrap_or("severity")));
        let mut extra = Vec::new();
        for (i, n) in names.iter().enumerate() {
            if !n.is_empty() && role_of(i).is_none() {
                extra.push((i, schema.extra_index(n)));
            }
        }

        let title = |name: &str| self.titles.get(&name.to_lowercase()).cloned();
        let mut columns: Vec<Column> = Vec::new();
        let mut push = |kind: ColumnKind, t: Option<String>| {
            if !columns.iter().any(|c| c.kind == kind) {
                columns.push(Column { kind, title: t });
            }
        };
        let field_column = |i: usize, schema: &Schema| match role_of(i) {
            Some(r) => (ColumnKind::Role(r), title(r.key())),
            None => {
                let e = extra.iter().find(|(f, _)| *f == i).map_or(0, |(_, e)| *e);
                let name = &schema.extra_names[e];
                (ColumnKind::Extra(e), Some(title(name).unwrap_or_else(|| name.clone())))
            }
        };
        match &self.columns {
            Some(list) => {
                for n in list {
                    if let Some(r) = Role::from_key(n) {
                        push(ColumnKind::Role(r), title(r.key()));
                    } else if let Some(i) = names.iter().position(|f| f.eq_ignore_ascii_case(n)) {
                        let (k, t) = field_column(i, schema);
                        push(k, t);
                    }
                }
            }
            None => {
                for (i, n) in names.iter().enumerate() {
                    if !n.is_empty() {
                        let (k, t) = field_column(i, schema);
                        push(k, t);
                    }
                }
            }
        }
        push(ColumnKind::Role(Role::Message), title("message"));
        schema.columns = columns;
        Mapping {
            roles,
            severity,
            extra,
            extra_len: schema.extra_names.len(),
        }
    }

    fn severity_of(&self, value: &str) -> Severity {
        let v = value.trim();
        if self.sev_error.is_match(v) {
            Severity::Error
        } else if self.sev_warning.is_match(v) {
            Severity::Warning
        } else {
            Severity::Info
        }
    }

    /// Converts the date/time text to the ISO form used everywhere ("YYYY-MM-DD HH:MM:SS.mmm").
    /// Text that does not parse is kept as it is.
    fn parse_datetime(&self, s: &str) -> String {
        if s.is_empty() {
            return String::new();
        }
        let formats = self.dt_formats.iter().map(String::as_str).chain(DEFAULT_DT_FORMATS.iter().copied());
        for f in formats {
            if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, f) {
                let dt = if self.utc {
                    use chrono::TimeZone;
                    chrono::Local.from_utc_datetime(&dt).naive_local()
                } else {
                    dt
                };
                return dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
            }
        }
        s.to_owned()
    }
}

/// Which field fills what (computed from the field names).
#[derive(Clone, Debug)]
struct Mapping {
    roles: [Vec<usize>; 4],
    severity: Option<usize>,
    /// (field index, extra column index)
    extra: Vec<(usize, usize)>,
    extra_len: usize,
}

/// Result of parsing one line with a definition.
pub enum DefLine {
    /// Ignored line (comment, header, `skip`, blank line).
    Skip,
    Entry(LogEntry),
    /// The line does not match – handled according to `unmatched`.
    Unmatched,
}

/// Parsing state of a definition (the w3c columns can change with every `#Fields:` header).
#[derive(Clone, Debug)]
pub struct DefState {
    def: Arc<LogDef>,
    map: Mapping,
}

impl DefState {
    /// Starts parsing with a definition; resets `schema` to its columns.
    pub fn new(def: Arc<LogDef>, schema: &mut Schema) -> Self {
        *schema = Schema {
            name: def.name.clone(),
            columns: Vec::new(),
            extra_names: Vec::new(),
        };
        let names: Vec<String> = match &def.kind {
            Kind::Regex(re) => re.capture_names().map(|n| n.unwrap_or("").to_owned()).collect(),
            Kind::W3c => def.default_fields.clone(),
        };
        let map = def.mapping(&names, schema);
        Self { def, map }
    }

    /// Lines that do not match are appended to the previous entry.
    pub fn appends_unmatched(&self) -> bool {
        self.def.unmatched == Unmatched::Append
    }

    pub fn skips_unmatched(&self) -> bool {
        self.def.unmatched == Unmatched::Skip
    }

    pub fn parse(&mut self, line: &str, line_no: usize, kw: bool, schema: &mut Schema) -> DefLine {
        if line.trim().is_empty() {
            return DefLine::Skip;
        }
        let def = self.def.clone();
        if let Kind::W3c = def.kind {
            if let Some(rest) = line.strip_prefix("#Fields:") {
                let names: Vec<String> = rest.split_whitespace().map(str::to_owned).collect();
                self.map = def.mapping(&names, schema);
                return DefLine::Skip;
            }
            if line.starts_with('#') {
                return DefLine::Skip;
            }
        }
        if def.skip.as_ref().is_some_and(|re| re.is_match(line)) {
            return DefLine::Skip;
        }
        match &def.kind {
            Kind::W3c => {
                let vals: Vec<&str> = line.split(' ').collect();
                DefLine::Entry(self.build(line_no, line, kw, |i| {
                    vals.get(i).map(|v| if *v == "-" { "" } else { *v })
                }))
            }
            Kind::Regex(re) => match re.captures(line) {
                Some(c) => DefLine::Entry(self.build(line_no, line, kw, |i| c.get(i).map(|m| m.as_str()))),
                None => DefLine::Unmatched,
            },
        }
    }

    fn build<'a>(
        &self,
        line_no: usize,
        line: &str,
        kw: bool,
        get: impl Fn(usize) -> Option<&'a str>,
    ) -> LogEntry {
        let join = |fields: &[usize]| -> String {
            let mut out = String::new();
            for v in fields.iter().filter_map(|&i| get(i)).filter(|v| !v.is_empty()) {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(v);
            }
            out
        };
        let m = &self.map;
        let message = if m.roles[Role::Message as usize].is_empty() {
            line.to_owned()
        } else {
            join(&m.roles[Role::Message as usize])
        };
        let severity = match m.severity.and_then(&get) {
            Some(v) => self.def.severity_of(v),
            None => keyword_sev(&message, kw),
        };
        let mut extra = vec![String::new(); m.extra_len];
        for &(f, e) in &m.extra {
            if let Some(v) = get(f) {
                extra[e] = v.to_owned();
            }
        }
        LogEntry {
            line_no,
            component: join(&m.roles[Role::Component as usize]),
            datetime: self.def.parse_datetime(&join(&m.roles[Role::DateTime as usize])),
            thread: join(&m.roles[Role::Thread as usize]),
            severity,
            message,
            extra,
        }
    }
}

/// All loaded definitions and the errors of files that could not be used.
#[derive(Default)]
pub struct DefSet {
    pub defs: Vec<Arc<LogDef>>,
    pub errors: Vec<String>,
}

/// Folders searched for definition files, in order of increasing priority.
pub fn def_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("logdef")))
    {
        dirs.push(d);
    }
    if let Some(d) = user_def_dir() {
        dirs.push(d);
    }
    dirs
}

/// Per-user definitions folder (`%APPDATA%\RusTrace\logdef` on Windows).
pub fn user_def_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("RusTrace").join("logdef"))
}

/// Loads the built-in definitions and the `.def` files from `dirs` (later folders override
/// earlier ones and the built-in files with the same name).
pub fn load_from(dirs: &[PathBuf]) -> DefSet {
    let mut texts: BTreeMap<String, (String, String, Option<PathBuf>)> = BUILTIN
        .iter()
        .map(|(id, t)| (id.to_lowercase(), ((*id).to_owned(), (*t).to_owned(), None)))
        .collect();
    let mut set = DefSet::default();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()).is_none_or(|x| !x.eq_ignore_ascii_case("def")) {
                continue;
            }
            let Some(id) = p.file_name().and_then(|s| s.to_str()).map(str::to_owned) else { continue };
            match std::fs::read_to_string(&p) {
                Ok(t) => {
                    texts.insert(id.to_lowercase(), (id, t, Some(p)));
                }
                Err(err) => set.errors.push(format!("{}: {err}", p.display())),
            }
        }
    }
    for (id, text, path) in texts.into_values() {
        let shown = path.as_ref().map_or_else(|| id.clone(), |p| p.display().to_string());
        match LogDef::parse(&id, &text, path) {
            Ok(d) => set.defs.push(Arc::new(d)),
            Err(e) => set.errors.push(format!("{shown}: {e}")),
        }
    }
    set
}

pub fn load_all() -> DefSet {
    load_from(&def_dirs())
}

/// The definition for a file by its name (`files` patterns).
pub fn by_file_name(defs: &[Arc<LogDef>], path: &Path) -> Option<Arc<LogDef>> {
    let name = path.file_name()?.to_string_lossy();
    defs.iter().find(|d| d.matches_file(&name)).cloned()
}

/// The definition for a file by its first lines (`detect`).
pub fn by_content(defs: &[Arc<LogDef>], lines: &[&str]) -> Option<Arc<LogDef>> {
    defs.iter().find(|d| d.detects(lines)).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin(id: &str) -> Arc<LogDef> {
        let (_, text) = BUILTIN.iter().find(|(i, _)| *i == id).unwrap();
        Arc::new(LogDef::parse(id, text, None).unwrap())
    }

    /// Parses lines like the parser does (unmatched lines appended to the previous entry).
    fn parse_lines(def: Arc<LogDef>, lines: &[&str]) -> (Vec<LogEntry>, Schema) {
        let mut schema = Schema::default();
        let mut st = DefState::new(def, &mut schema);
        let mut out: Vec<LogEntry> = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            match st.parse(l, i + 1, true, &mut schema) {
                DefLine::Entry(e) => out.push(e),
                DefLine::Unmatched if st.appends_unmatched() && !out.is_empty() => {
                    let m = &mut out.last_mut().unwrap().message;
                    m.push('\n');
                    m.push_str(l);
                }
                _ => {}
            }
        }
        (out, schema)
    }

    fn extra<'a>(e: &'a LogEntry, s: &Schema, name: &str) -> &'a str {
        let i = s.extra_names.iter().position(|n| n == name).unwrap();
        e.extra.get(i).map_or("", String::as_str)
    }

    #[test]
    fn builtin_definitions_parse() {
        let set = load_from(&[]);
        assert!(set.errors.is_empty(), "{:?}", set.errors);
        let ids: Vec<&str> = set.defs.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["cbs.log.def", "dism.log.def", "iisweb.log.def"]);
    }

    #[test]
    fn cbs_log() {
        let (e, s) = parse_lines(
            builtin("cbs.log.def"),
            &[
                "2026-10-23 10:12:33, Info                  CBS    Starting TrustedInstaller initialization.",
                "2026-10-23 10:12:34, Error                 CSI    00000012 (F) STATUS_SXS_COMPONENT_STORE_CORRUPT [HRESULT = 0x80073712]",
                "   continuation of the dump",
                "",
                "2026-10-23 10:12:35, Warning               CBS",
            ],
        );
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].datetime, "2026-10-23 10:12:33.000");
        assert_eq!(e[0].component, "CBS");
        assert_eq!(e[0].message, "Starting TrustedInstaller initialization.");
        assert_eq!(e[0].severity, Severity::Info);
        assert_eq!(e[1].severity, Severity::Error);
        assert!(e[1].message.ends_with("0x80073712]\n   continuation of the dump"));
        assert_eq!(e[2].severity, Severity::Warning);
        assert_eq!(e[2].message, "");
        assert_eq!(extra(&e[1], &s, "severity"), "Error");
        let kinds: Vec<ColumnKind> = s.columns.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            [
                ColumnKind::Role(Role::DateTime),
                ColumnKind::Extra(0),
                ColumnKind::Role(Role::Component),
                ColumnKind::Role(Role::Message)
            ]
        );
        assert_eq!(s.columns[1].title.as_deref(), Some("Level"));
    }

    #[test]
    fn dism_log() {
        let (e, s) = parse_lines(
            builtin("dism.log.def"),
            &[
                "2026-10-23 10:20:01, Info                  DISM   PID=4312 TID=1188 Scratch directory set to 'C:\\Temp\\'. - CDISMManager::put_ScratchDir",
                "2026-10-23 10:20:02, Error                 DISM   DISM Package Manager: PID=4312 TID=1188 Failed finalizing changes. - CDISMPackageManager::Internal_Finalize(hr:0x800f081f)",
                "2026-10-23 10:20:03, Info                  DISM   DISM.EXE: <----- Starting Dism.exe session ----->",
            ],
        );
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].thread, "1188");
        assert_eq!(e[0].message, "Scratch directory set to 'C:\\Temp\\'.");
        assert_eq!(extra(&e[0], &s, "function"), "CDISMManager::put_ScratchDir");
        assert_eq!(extra(&e[0], &s, "provider"), "");
        assert_eq!(extra(&e[0], &s, "pid"), "4312");
        assert_eq!(e[1].severity, Severity::Error);
        assert_eq!(extra(&e[1], &s, "provider"), "DISM Package Manager");
        assert_eq!(e[1].message, "Failed finalizing changes.");
        assert_eq!(
            extra(&e[1], &s, "function"),
            "CDISMPackageManager::Internal_Finalize(hr:0x800f081f)"
        );
        assert_eq!(e[2].message, "DISM.EXE: <----- Starting Dism.exe session ----->");
        assert_eq!(e[2].thread, "");
    }

    #[test]
    fn iis_w3c_log() {
        let def = builtin("iisweb.log.def");
        assert!(def.matches_file("u_ex261023.log"));
        assert!(!def.matches_file("AppEnforce.log"));
        let header = [
            "#Software: Microsoft Internet Information Services 10.0",
            "#Version: 1.0",
        ];
        assert!(def.detects(&header));
        let (e, s) = parse_lines(
            def,
            &[
                header[0],
                header[1],
                "#Fields: date time s-ip cs-method cs-uri-stem cs-uri-query sc-status time-taken",
                "2026-10-23 08:00:01 10.0.0.5 GET /default.aspx id=5 500 15",
                "2026-10-23 08:00:02 10.0.0.5 POST /api/x - 404 3",
                "2026-10-23 08:00:03 10.0.0.5 GET /ok - 200 1",
            ],
        );
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].message, "GET /default.aspx id=5");
        assert_eq!(e[1].message, "POST /api/x");
        assert_eq!(
            [e[0].severity, e[1].severity, e[2].severity],
            [Severity::Error, Severity::Warning, Severity::Info]
        );
        assert_eq!(e[0].line_no, 4);
        assert_eq!(extra(&e[0], &s, "sc-status"), "500");
        assert_eq!(extra(&e[0], &s, "time-taken"), "15");
        // UTC converted to local time.
        use chrono::TimeZone;
        let utc = chrono::NaiveDateTime::parse_from_str("2026-10-23 08:00:01", "%Y-%m-%d %H:%M:%S").unwrap();
        let local = chrono::Local.from_utc_datetime(&utc).naive_local();
        assert_eq!(e[0].datetime, local.format("%Y-%m-%d %H:%M:%S%.3f").to_string());
        // The header defines the columns; fields used by roles are not separate columns.
        let titles: Vec<&str> = s.columns.iter().filter_map(|c| c.title.as_deref()).collect();
        assert_eq!(titles, ["Server IP", "Request", "Status", "Time (ms)"]);
        assert_eq!(s.columns[0].kind, ColumnKind::Role(Role::DateTime));
    }

    #[test]
    fn definition_errors_and_wildcards() {
        assert!(LogDef::parse("a.def", "pattern = (unclosed", None).is_err());
        assert!(LogDef::parse("a.def", "pattern = ^(.*)$", None).is_err()); // no named group
        assert!(LogDef::parse("a.def", "patern = ^(?P<message>.*)$", None).is_err());
        let d = LogDef::parse("my.log.def", "pattern = ^(?P<message>.*)$", None).unwrap();
        assert!(d.matches_file("MY.LOG"));
        assert_eq!(d.name, "my.log");
        assert!(wildcard("u_ex*.log", "u_ex261023.log"));
        assert!(wildcard("a?c*", "abcdef"));
        assert!(!wildcard("cbs*.log", "cbs.txt"));
    }
}
