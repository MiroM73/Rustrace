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
//! | `separator` | w3c: `space` (default), `tab` or one character, e.g. `,` – other than space: CSV-style values (`"…"` quoting, `""` = `"`), `-` stays a value |
//! | `pattern` | regex for one entry (format `regex`); every **named group** is a column |
//! | `fields` | w3c: column names used until a `#Fields:` header is found |
//! | `skip` | regex; matching lines are ignored |
//! | `unmatched` | lines not matching `pattern`: `append` to the previous entry (default for `regex`), `entry` = own entry (default for `w3c`), `skip` |
//! | `role.message`, `role.component`, `role.thread`, `role.datetime` | columns (one or more, joined by a space) that fill the standard columns; default: the column with that name (`datetime` also `date` + `time`) |
//! | `role.severity` | column whose value gives the severity; default: `severity` |
//! | `severity.error`, `severity.warning` | regexes (case-insensitive) for the severity value |
//! | `datetime.format` | chrono format(s) of the date/time, separated by `\|`, e.g. `%Y-%m-%d %H:%M:%S` |
//! | `timezone` | `local` (default) or `utc` (converted to local time) |
//! | `columns` | order of the columns in the table (role names or column names, `*` = all the other columns in the order of the pattern / header); columns not listed are hidden unless `*` is used; default: order in the pattern / header |
//! | `title.<column>` | column header (`title.message = Request`) |
//! | `http_status`, `http_substatus`, `win32_status` | columns with an HTTP status, an IIS substatus and a Win32 status code (decimal) – explained in the details pane, in the cell tooltip and in the error code lookup (#28); optional, a missing column is ignored |
//!
//! Columns that are not used by `role.message/component/thread/datetime` are kept as extra
//! columns (shown in the table, searchable). Definitions are built in (`logdef/*.def` in the
//! source tree) and loaded **without recompiling** from `logdef\` next to the executable (a file
//! with the same name replaces the built-in one) and from `<config dir>/RusTrace/logdef/` (the
//! user's own definitions – they replace nothing, see `load_from`).

use crate::parser::{LogEntry, Severity, keyword_sev};
use regex::{Regex, RegexBuilder};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Built-in definitions (file name, content).
const BUILTIN: &[(&str, &str)] = &[
    ("cbs.log.def", include_str!("../logdef/cbs.log.def")),
    ("dism.log.def", include_str!("../logdef/dism.log.def")),
    (
        "exchconnect.log.def",
        include_str!("../logdef/exchconnect.log.def"),
    ),
    ("iisweb.log.def", include_str!("../logdef/iisweb.log.def")),
];

/// Date/time formats tried when `datetime.format` does not match.
const DEFAULT_DT_FORMATS: &[&str] = &[
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y/%m/%d %H:%M:%S%.f",
];

/// Standard columns every log has (filled by the CMTrace parser or by a definition's roles).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, serde::Serialize, serde::Deserialize)]
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
        Role::ALL
            .into_iter()
            .find(|r| r.key().eq_ignore_ascii_case(s))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, serde::Serialize, serde::Deserialize)]
pub enum ColumnKind {
    Role(Role),
    /// Index into `LogEntry::extra` / `Schema::extra_names`.
    Extra(usize),
    /// The file of the entry – added by the UI when the log is merged from rotated parts.
    File,
    /// Time since the previous / reference row – added by the UI when turned on (#23).
    Delta,
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
    /// Columns with status codes the UI explains (#28).
    pub status: StatusFields,
}

/// Names of the columns with status codes (`.def` keys `http_status`, `http_substatus`,
/// `win32_status`, #28). Only extra columns are explained (not columns used by a role).
#[derive(Clone, PartialEq, Eq, Debug, Hash, Default)]
pub struct StatusFields {
    pub http: Option<String>,
    pub substatus: Option<String>,
    pub win32: Option<String>,
}

/// Extra column indexes of the status columns in a schema (`StatusFields::resolve`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StatusColumns {
    pub http: Option<usize>,
    pub substatus: Option<usize>,
    pub win32: Option<usize>,
}

impl StatusFields {
    pub fn is_empty(&self) -> bool {
        self.http.is_none() && self.substatus.is_none() && self.win32.is_none()
    }

    /// Finds the columns in the extra column names (case-insensitive); missing ones are `None`.
    pub fn resolve(&self, extra_names: &[String]) -> StatusColumns {
        let find = |n: &Option<String>| {
            n.as_ref()
                .and_then(|n| extra_names.iter().position(|x| x.eq_ignore_ascii_case(n)))
        };
        StatusColumns {
            http: find(&self.http),
            substatus: find(&self.substatus),
            win32: find(&self.win32),
        }
    }
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
            status: StatusFields::default(),
        }
    }
}

impl Schema {
    fn extra_index(&mut self, name: &str) -> usize {
        match self
            .extra_names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
        {
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
    /// W3C-like log: column names from the `#Fields:` header, `None` = space-separated
    /// (IIS, `-` = empty), `Some(c)` = values separated by `c`, CSV-style (Exchange).
    W3c(Option<char>),
}

/// Splits a w3c data line into its values.
///
/// Space-separated (IIS): `-` is an empty value. Other separators (Exchange CSV logs): a value
/// may be quoted (`"a, b"`, `""` inside = `"`), so it can contain the separator; `-` is kept
/// (in Exchange logs it is a real value, e.g. `direction` `-` = disconnect).
fn split_values(line: &str, sep: Option<char>) -> Vec<Cow<'_, str>> {
    let Some(sep) = sep else {
        return line
            .split(' ')
            .map(|v| Cow::Borrowed(if v == "-" { "" } else { v }))
            .collect();
    };
    let line = line.trim_end_matches(['\r', '\n']);
    let mut out = Vec::new();
    let mut rest = line;
    loop {
        if let Some(q) = rest.strip_prefix('"') {
            // Quoted value: up to the closing quote that is not doubled.
            let mut val = String::new();
            let mut chars = q.char_indices().peekable();
            let mut end = q.len();
            while let Some((i, c)) = chars.next() {
                if c == '"' {
                    if chars.peek().is_some_and(|&(_, n)| n == '"') {
                        val.push('"');
                        chars.next();
                    } else {
                        end = i + 1;
                        break;
                    }
                } else {
                    val.push(c);
                }
            }
            // Text between the closing quote and the next separator (malformed) is kept.
            let after = &q[end.min(q.len())..];
            match after.find(sep) {
                Some(i) => {
                    val.push_str(&after[..i]);
                    out.push(Cow::Owned(val));
                    rest = &after[i + sep.len_utf8()..];
                }
                None => {
                    val.push_str(after);
                    out.push(Cow::Owned(val));
                    break;
                }
            }
        } else {
            match rest.find(sep) {
                Some(i) => {
                    out.push(Cow::Borrowed(&rest[..i]));
                    rest = &rest[i + sep.len_utf8()..];
                }
                None => {
                    out.push(Cow::Borrowed(rest));
                    break;
                }
            }
        }
    }
    out
}

/// A parsed log definition.
#[derive(Debug)]
pub struct LogDef {
    /// File name, e.g. `cbs.log.def` – identifies the definition.
    pub id: String,
    pub name: String,
    /// Where it was loaded from (`None` = built-in).
    pub path: Option<PathBuf>,
    /// Where it comes from: 0 = built-in, then the folders of `load_from` in their order
    /// (higher wins when several definitions match the same file name).
    pub priority: u8,
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
    /// Columns with status codes (#28).
    status: StatusFields,
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
                    | "separator"
                    | "skip"
                    | "unmatched"
                    | "severity.error"
                    | "severity.warning"
                    | "datetime.format"
                    | "timezone"
                    | "columns"
                    | "http_status"
                    | "http_substatus"
                    | "win32_status"
            ) {
                return Err(format!("unknown key \"{k}\""));
            }
        }

        let get = |k: &str| map.get(k).map(String::as_str).filter(|v| !v.is_empty());
        let separator = match get("separator") {
            None => None,
            Some(v) if v.eq_ignore_ascii_case("space") || v == " " => None,
            Some(v) if v.eq_ignore_ascii_case("tab") || v == "\\t" => Some('\t'),
            Some(v) => {
                let mut c = v.chars();
                match (c.next(), c.next()) {
                    (Some(ch), None) if ch != '"' => Some(ch),
                    _ => return Err(format!("separator: \"{v}\" (space, tab or one character)")),
                }
            }
        };
        let kind = match get("format")
            .unwrap_or("regex")
            .to_ascii_lowercase()
            .as_str()
        {
            "w3c" => Kind::W3c(separator),
            _ if get("separator").is_some() => {
                return Err("separator: only for format = w3c".into());
            }
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
        let default_unmatched = if matches!(kind, Kind::W3c(_)) {
            "entry"
        } else {
            "append"
        };
        let unmatched = match get("unmatched")
            .unwrap_or(default_unmatched)
            .to_ascii_lowercase()
            .as_str()
        {
            "append" => Unmatched::Append,
            "entry" => Unmatched::Entry,
            "skip" => Unmatched::Skip,
            other => {
                return Err(format!(
                    "unmatched: unknown value \"{other}\" (append, entry, skip)"
                ));
            }
        };
        let utc = match get("timezone")
            .unwrap_or("local")
            .to_ascii_lowercase()
            .as_str()
        {
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
            priority: 0,
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
                .map(|f| {
                    f.split('|')
                        .map(|x| x.trim().to_owned())
                        .filter(|x| !x.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            utc,
            columns: get("columns").map(list),
            titles,
            default_fields: get("fields").map(list).unwrap_or_default(),
            status: StatusFields {
                http: get("http_status").map(str::to_owned),
                substatus: get("http_substatus").map(str::to_owned),
                win32: get("win32_status").map(str::to_owned),
            },
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
    /// the table columns of `schema` – with `merge` the columns that are already there stay and
    /// only new ones are added (a later w3c `#Fields:` header with other fields: the table must
    /// show the columns of every part of the file).
    fn mapping(&self, names: &[String], schema: &mut Schema, merge: bool) -> Mapping {
        let roles = Role::ALL.map(|r| self.role_fields(r, names));
        let role_of = |i: usize| {
            Role::ALL
                .into_iter()
                .find(|r| roles[*r as usize].contains(&i))
        };
        let severity = names.iter().position(|f| {
            f.eq_ignore_ascii_case(self.severity_field.as_deref().unwrap_or("severity"))
        });
        let mut extra = Vec::new();
        for (i, n) in names.iter().enumerate() {
            if !n.is_empty() && role_of(i).is_none() {
                extra.push((i, schema.extra_index(n)));
            }
        }

        let title = |name: &str| self.titles.get(&name.to_lowercase()).cloned();
        let mut columns: Vec<Column> = if merge {
            schema.columns.clone()
        } else {
            Vec::new()
        };
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
                (
                    ColumnKind::Extra(e),
                    Some(title(name).unwrap_or_else(|| name.clone())),
                )
            }
        };
        match &self.columns {
            Some(list) => {
                let named = |n: &str| -> Option<(ColumnKind, Option<String>)> {
                    if let Some(r) = Role::from_key(n) {
                        Some((ColumnKind::Role(r), title(r.key())))
                    } else {
                        let i = names.iter().position(|f| f.eq_ignore_ascii_case(n))?;
                        Some(field_column(i, schema))
                    }
                };
                // `*` = every column not named in the list, in the order of the fields (#41).
                let listed: Vec<ColumnKind> = list
                    .iter()
                    .filter_map(|n| named(n))
                    .map(|(k, _)| k)
                    .collect();
                for n in list {
                    if n == "*" {
                        for (i, _) in names.iter().enumerate().filter(|(_, f)| !f.is_empty()) {
                            let (k, t) = field_column(i, schema);
                            if !listed.contains(&k) {
                                push(k, t);
                            }
                        }
                    } else if let Some((k, t)) = named(n) {
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
        let formats = self
            .dt_formats
            .iter()
            .map(String::as_str)
            .chain(DEFAULT_DT_FORMATS.iter().copied());
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
    /// A w3c `#Fields:` header was found (later headers add columns instead of replacing them).
    header_seen: bool,
    /// An entry was parsed (the extra column indexes are in use).
    entries_parsed: bool,
}

impl DefState {
    /// Starts parsing with a definition; resets `schema` to its columns.
    pub fn new(def: Arc<LogDef>, schema: &mut Schema) -> Self {
        *schema = Schema {
            name: def.name.clone(),
            columns: Vec::new(),
            extra_names: Vec::new(),
            status: def.status.clone(),
        };
        let names: Vec<String> = match &def.kind {
            Kind::Regex(re) => re
                .capture_names()
                .map(|n| n.unwrap_or("").to_owned())
                .collect(),
            Kind::W3c(_) => def.default_fields.clone(),
        };
        let map = def.mapping(&names, schema, false);
        Self {
            def,
            map,
            header_seen: false,
            entries_parsed: false,
        }
    }

    /// The definition in use.
    pub fn def(&self) -> &Arc<LogDef> {
        &self.def
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
        if let Kind::W3c(sep) = def.kind {
            if let Some(rest) = line.strip_prefix("#Fields:") {
                let names: Vec<String> = match sep {
                    None => rest.split_whitespace().map(str::to_owned).collect(),
                    Some(c) => rest
                        .split(c)
                        .map(|n| n.trim().trim_matches('"').to_owned())
                        .filter(|n| !n.is_empty())
                        .collect(),
                };
                // The first header replaces the default fields of the definition (and their
                // extra columns, while no entry uses them); later headers only add columns.
                if !self.header_seen && !self.entries_parsed {
                    schema.extra_names.clear();
                }
                self.map = def.mapping(&names, schema, self.header_seen);
                self.header_seen = true;
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
            Kind::W3c(sep) => {
                self.entries_parsed = true;
                let vals = split_values(line, *sep);
                DefLine::Entry(self.build(line_no, line, kw, |i| vals.get(i).map(|v| v.as_ref())))
            }
            Kind::Regex(re) => match re.captures(line) {
                Some(c) => {
                    DefLine::Entry(self.build(line_no, line, kw, |i| c.get(i).map(|m| m.as_str())))
                }
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
            for v in fields
                .iter()
                .filter_map(|&i| get(i))
                .filter(|v| !v.is_empty())
            {
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
            datetime: self
                .def
                .parse_datetime(&join(&m.roles[Role::DateTime as usize])),
            thread: join(&m.roles[Role::Thread as usize]),
            severity,
            message,
            extra,
            raw: (0, 0),
        }
    }
}

/// All loaded definitions and the errors of files that could not be used.
#[derive(Default)]
pub struct DefSet {
    pub defs: Vec<Arc<LogDef>>,
    pub errors: Vec<String>,
    /// Texts of the predefined definitions (built-in, or their copy in `logdef\` next to the
    /// program), lower-case file name → text – for the "new / changed definitions" notice.
    pub predefined: BTreeMap<String, String>,
    /// The user's own definitions: lower-case file name → (path, text).
    pub own: BTreeMap<String, (PathBuf, String)>,
}

/// `LogDef::priority` of the built-in definitions, the folder next to the program and the
/// user's folder (higher wins when several definitions match the same file name).
pub const PRIO_BUILTIN: u8 = 0;
pub const PRIO_PROGRAM: u8 = 1;
pub const PRIO_USER: u8 = 2;

/// Which definitions are used: predefined (built-in + `logdef\` next to the program), the
/// user's own ones, or both.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceFilter {
    All,
    Predefined,
    Own,
}

impl SourceFilter {
    pub fn from_key(s: &str) -> Self {
        match s {
            "predefined" => SourceFilter::Predefined,
            "own" => SourceFilter::Own,
            _ => SourceFilter::All,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SourceFilter::All => "all",
            SourceFilter::Predefined => "predefined",
            SourceFilter::Own => "own",
        }
    }
}

impl LogDef {
    /// The same definition (file name and source) – e.g. after the definitions were reloaded.
    pub fn same_as(&self, other: &LogDef) -> bool {
        self.id.eq_ignore_ascii_case(&other.id) && self.priority == other.priority
    }

    /// A definition from the user's folder.
    pub fn is_own(&self) -> bool {
        self.priority == PRIO_USER
    }
}

/// `.def` files of a folder: (file name, text, path); unreadable files go to `errors`.
fn read_def_files(dir: &Path, errors: &mut Vec<String>) -> Vec<(String, String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension()
            .and_then(|x| x.to_str())
            .is_none_or(|x| !x.eq_ignore_ascii_case("def"))
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

// ----------------------------------------------------------------------
// New / changed predefined definitions
// ----------------------------------------------------------------------

/// `key = value` pairs of a definition text (keys lower-case, comments ignored).
pub fn key_values(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .map(|l| l.trim_start_matches('\u{feff}').trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect()
}

/// One key of a predefined definition that changed between two versions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyChange {
    pub key: String,
    /// Value in the version seen before (`None` = the key is new).
    pub old: Option<String>,
    /// Value now (`None` = the key was removed).
    pub new: Option<String>,
    /// Value in the user's own copy with the same file name (`None` there = no such key);
    /// `None` here = the user has no own copy.
    pub own: Option<Option<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateKind {
    New,
    Changed,
    Removed,
}

/// A predefined definition that is new, changed or removed since the version the user saw.
#[derive(Clone, Debug)]
pub struct DefUpdate {
    pub id: String,
    pub name: String,
    pub kind: UpdateKind,
    pub changes: Vec<KeyChange>,
    /// The user's own copy with the same file name.
    pub own_path: Option<PathBuf>,
}

/// Compares the predefined definitions with the versions the user has seen (`known`, file name
/// → text) and describes what is new, changed (key by key, with the value in the user's own
/// copy) or removed.
pub fn definition_updates(
    known: &BTreeMap<String, String>,
    predefined: &BTreeMap<String, String>,
    own: &BTreeMap<String, (PathBuf, String)>,
) -> Vec<DefUpdate> {
    let name_of = |id: &str, text: &str| {
        key_values(text)
            .remove("name")
            .unwrap_or_else(|| id.to_owned())
    };
    let mut out = Vec::new();
    for (id, text) in predefined {
        let own_copy = own.get(id);
        let own_kv = own_copy.map(|(_, t)| key_values(t));
        let kind = match known.get(id) {
            None => UpdateKind::New,
            Some(old) if key_values(old) == key_values(text) => continue,
            Some(_) => UpdateKind::Changed,
        };
        let changes = match known.get(id) {
            Some(old) => {
                let (old, new) = (key_values(old), key_values(text));
                let keys: std::collections::BTreeSet<&String> =
                    old.keys().chain(new.keys()).collect();
                keys.into_iter()
                    .filter(|k| old.get(*k) != new.get(*k))
                    .map(|k| KeyChange {
                        key: k.clone(),
                        old: old.get(k).cloned(),
                        new: new.get(k).cloned(),
                        own: own_kv.as_ref().map(|o| o.get(k).cloned()),
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        out.push(DefUpdate {
            id: id.clone(),
            name: name_of(id, text),
            kind,
            changes,
            own_path: own_copy.map(|(p, _)| p.clone()),
        });
    }
    for (id, text) in known {
        if !predefined.contains_key(id) {
            out.push(DefUpdate {
                id: id.clone(),
                name: name_of(id, text),
                kind: UpdateKind::Removed,
                changes: Vec::new(),
                own_path: own.get(id).map(|(p, _)| p.clone()),
            });
        }
    }
    out
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

/// Loads the definitions. `dirs` = [folder next to the program, user's folder] (see `def_dirs`).
///
/// * predefined = the built-in definitions; a file with the same name in the folder next to the
///   program replaces the built-in one (that folder holds the copies shipped with the program),
/// * own = the files in the user's folder – they replace nothing, so a predefined and an own
///   definition with the same file name are both available,
/// * `filter` chooses which of the two groups is used.
///
/// The result is ordered by priority (own, next to the program, built-in), then by file name, so
/// when several definitions match a file the user's own one wins.
pub fn load_from(dirs: &[PathBuf], filter: SourceFilter) -> DefSet {
    let mut set = DefSet::default();
    let mut predefined: BTreeMap<String, (String, String, Option<PathBuf>, u8)> = BUILTIN
        .iter()
        .map(|(id, t)| {
            (
                id.to_lowercase(),
                ((*id).to_owned(), (*t).to_owned(), None, PRIO_BUILTIN),
            )
        })
        .collect();
    let mut errors = Vec::new();
    if let Some(dir) = dirs.first() {
        for (id, text, path) in read_def_files(dir, &mut errors) {
            predefined.insert(id.to_lowercase(), (id, text, Some(path), PRIO_PROGRAM));
        }
    }
    let mut own: BTreeMap<String, (String, String, Option<PathBuf>, u8)> = BTreeMap::new();
    let mut own_errors = Vec::new();
    if let Some(dir) = dirs.get(1) {
        for (id, text, path) in read_def_files(dir, &mut own_errors) {
            own.insert(id.to_lowercase(), (id, text, Some(path), PRIO_USER));
        }
    }
    set.predefined = predefined
        .iter()
        .map(|(k, (_, t, _, _))| (k.clone(), t.clone()))
        .collect();
    set.own = own
        .iter()
        .filter_map(|(k, (_, t, p, _))| p.clone().map(|p| (k.clone(), (p, t.clone()))))
        .collect();

    let mut groups = Vec::new();
    if filter != SourceFilter::Own {
        set.errors.append(&mut errors);
        groups.push(predefined);
    }
    if filter != SourceFilter::Predefined {
        set.errors.append(&mut own_errors);
        groups.push(own);
    }
    for (id, text, path, priority) in groups.into_iter().flat_map(BTreeMap::into_values) {
        let shown = path
            .as_ref()
            .map_or_else(|| id.clone(), |p| p.display().to_string());
        match LogDef::parse(&id, &text, path) {
            Ok(d) => set.defs.push(Arc::new(LogDef { priority, ..d })),
            Err(e) => set.errors.push(format!("{shown}: {e}")),
        }
    }
    set.defs.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.id.to_lowercase().cmp(&b.id.to_lowercase()))
    });
    set
}

pub fn load_all(filter: SourceFilter) -> DefSet {
    load_from(&def_dirs(), filter)
}

/// The definition for a file by its name (`files` patterns); the first of `matching_by_name`.
pub fn by_file_name(defs: &[Arc<LogDef>], path: &Path) -> Option<Arc<LogDef>> {
    matching_by_name(defs, path).into_iter().next()
}

/// All definitions whose `files` patterns match the file name, in priority order.
pub fn matching_by_name(defs: &[Arc<LogDef>], path: &Path) -> Vec<Arc<LogDef>> {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy()) else {
        return Vec::new();
    };
    defs.iter()
        .filter(|d| d.matches_file(&name))
        .cloned()
        .collect()
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
        let set = load_from(&[], SourceFilter::All);
        assert!(set.errors.is_empty(), "{:?}", set.errors);
        let ids: Vec<&str> = set.defs.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "cbs.log.def",
                "dism.log.def",
                "exchconnect.log.def",
                "iisweb.log.def"
            ]
        );
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
        assert!(
            e[1].message
                .ends_with("0x80073712]\n   continuation of the dump")
        );
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
        assert_eq!(
            e[2].message,
            "DISM.EXE: <----- Starting Dism.exe session ----->"
        );
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
        // Status columns (#28): only those in the header are found.
        let st = s.status.resolve(&s.extra_names);
        assert_eq!(
            st.http.map(|i| s.extra_names[i].as_str()),
            Some("sc-status")
        );
        assert_eq!((st.substatus, st.win32), (None, None));
        // UTC converted to local time.
        use chrono::TimeZone;
        let utc = chrono::NaiveDateTime::parse_from_str("2026-10-23 08:00:01", "%Y-%m-%d %H:%M:%S")
            .unwrap();
        let local = chrono::Local.from_utc_datetime(&utc).naive_local();
        assert_eq!(
            e[0].datetime,
            local.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
        );
        // The header defines the columns; fields used by roles are not separate columns.
        let titles: Vec<&str> = s
            .columns
            .iter()
            .filter_map(|c| c.title.as_deref())
            .collect();
        assert_eq!(titles, ["Status", "Time (ms)", "Request", "Server IP"]);
        assert_eq!(s.columns[0].kind, ColumnKind::Role(Role::DateTime));
    }

    /// Built-in IIS columns (#41): status columns first, every other field of any `#Fields:` set
    /// after the request (`*`), none lost.
    #[test]
    fn iis_status_columns_first() {
        let titles = |lines: &[&str]| -> Vec<String> {
            let (_, s) = parse_lines(builtin("iisweb.log.def"), lines);
            s.columns
                .iter()
                .map(|c| c.title.clone().unwrap_or_else(|| format!("{:?}", c.kind)))
                .collect()
        };
        // The default IIS field set (no header yet).
        assert_eq!(
            titles(&[]),
            [
                "Role(DateTime)",
                "Status",
                "Substatus",
                "Win32 status",
                "Time (ms)",
                "Client IP",
                "User",
                "Request",
                "Server IP",
                "Port",
                "User agent",
                "Referer"
            ]
        );
        // Fields the definition does not name stay visible in their header order.
        assert_eq!(
            titles(&[
                "#Fields: date time s-sitename s-computername s-ip cs-method cs-uri-stem cs-uri-query s-port cs-username c-ip cs-version cs(User-Agent) cs(Cookie) cs(Referer) cs-host sc-status sc-substatus sc-win32-status sc-bytes cs-bytes time-taken",
                "2026-10-23 08:00:01 W3SVC1 MP01 10.0.0.5 GET /a - 443 - 10.0.0.9 HTTP/1.1 x - - mp01 200 0 0 100 200 15",
            ]),
            [
                "Role(DateTime)",
                "Status",
                "Substatus",
                "Win32 status",
                "Time (ms)",
                "Client IP",
                "User",
                "Request",
                "s-sitename",
                "s-computername",
                "Server IP",
                "Port",
                "cs-version",
                "User agent",
                "cs(Cookie)",
                "Referer",
                "cs-host",
                "sc-bytes",
                "cs-bytes"
            ]
        );
    }

    /// `columns` without `*` hides the columns it does not name (own definitions rely on it).
    #[test]
    fn columns_without_star_hide_the_rest() {
        let def = LogDef::parse(
            "x.def",
            "pattern = ^(?P<a>\\S+) (?P<b>\\S+) (?P<message>.*)$\ncolumns = b message\n",
            None,
        )
        .unwrap();
        let (e, s) = parse_lines(Arc::new(def), &["1 2 text"]);
        let titles: Vec<&str> = s
            .columns
            .iter()
            .filter_map(|c| c.title.as_deref())
            .collect();
        assert_eq!(titles, ["b"]);
        assert_eq!(s.columns.len(), 2);
        assert_eq!(extra(&e[0], &s, "a"), "1");
    }

    /// Exchange Transport Connectivity log (issue #13): comma-separated `#Fields:` and values,
    /// one date-time field with `T` and `Z`, `-` kept as a value, quoted values with commas.
    #[test]
    fn exchange_connectivity_log() {
        let def = builtin("exchconnect.log.def");
        assert!(def.matches_file("CONNECTLOG20260927-1.LOG"));
        let header = [
            "#Software: Microsoft Exchange Server",
            "#Version: 15.0.0.0",
            "#Log-type: Transport Connectivity Log",
            "#Date: 2026-09-27T00:00:25.076Z",
            "#Fields: date-time,session,source,Destination,direction,description",
        ];
        assert!(def.detects(&header));
        assert!(!builtin("iisweb.log.def").detects(&header));
        let mut lines = header.to_vec();
        lines.extend([
            "2026-09-26T23:59:49.511Z,08DF1042B9A7211F,SMTP,shadowredundancy,+,ShadowRedundancy 00000000-0000-0000-0000-000000000000;QueueLength=<no priority counts>. ",
            "2026-09-26T23:59:50.001Z,08DF1042B9A7211F,SMTP,shadowredundancy,-,,",
            "2026-09-26T23:59:51.002Z,08DF1042B9A72120,SMTP,mail.contoso.com,>,\"Failed connection to 10.0.0.5:25 (TimedOut:10060, \"\"x\"\")\"",
            "2026-09-26T23:59:52.003Z,08DF1042B9A72121,SMTP,mail.contoso.com,*,Retrying the connection",
        ]);
        let (e, s) = parse_lines(def, &lines);
        assert_eq!(e.len(), 4);
        assert_eq!(e[0].line_no, 6);
        assert_eq!(e[0].component, "SMTP");
        assert!(e[0].message.starts_with("ShadowRedundancy 0000"));
        assert_eq!(extra(&e[0], &s, "session"), "08DF1042B9A7211F");
        assert_eq!(extra(&e[0], &s, "Destination"), "shadowredundancy");
        assert_eq!(extra(&e[0], &s, "direction"), "+");
        // "-" is a real value here (disconnect), an empty value stays empty.
        assert_eq!(extra(&e[1], &s, "direction"), "-");
        assert_eq!(e[1].message, "");
        // A quoted value keeps its commas and doubled quotes.
        assert_eq!(
            e[2].message,
            "Failed connection to 10.0.0.5:25 (TimedOut:10060, \"x\")"
        );
        assert_eq!(
            [e[0].severity, e[2].severity, e[3].severity],
            [Severity::Info, Severity::Error, Severity::Warning]
        );
        // UTC with "T" and "Z" converted to local time.
        use chrono::TimeZone;
        let utc = chrono::NaiveDateTime::parse_from_str(
            "2026-09-26 23:59:49.511",
            "%Y-%m-%d %H:%M:%S%.3f",
        )
        .unwrap();
        let local = chrono::Local.from_utc_datetime(&utc).naive_local();
        assert_eq!(
            e[0].datetime,
            local.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
        );
        let titles: Vec<&str> = s
            .columns
            .iter()
            .filter_map(|c| c.title.as_deref())
            .collect();
        assert_eq!(
            titles,
            ["Session", "Source", "Dir", "Destination", "Description"]
        );
        assert_eq!(s.columns[0].kind, ColumnKind::Role(Role::DateTime));
    }

    #[test]
    fn separator_values() {
        let v = |l: &str, s: Option<char>| {
            split_values(l, s)
                .into_iter()
                .map(|c| c.into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(v("a - c", None), ["a", "", "c"]);
        assert_eq!(
            v("a,-,,\"b,\"\"c\"\"\",d\r", Some(',')),
            ["a", "-", "", "b,\"c\"", "d"]
        );
        assert_eq!(v("x\ty", Some('\t')), ["x", "y"]);
        assert_eq!(v("\"unclosed,z", Some(',')), ["unclosed,z"]);
        let def = |extra: &str| LogDef::parse("t.def", &format!("name = T\n{extra}"), None);
        assert!(def("format = w3c\nseparator = ;").is_ok());
        assert!(def("format = w3c\nseparator = tab").is_ok());
        assert!(def("format = w3c\nseparator = ab").is_err());
        assert!(def("pattern = (?P<message>.*)\nseparator = ,").is_err());
    }

    /// Status column keys (#28) are optional; a named column that does not exist is ignored.
    #[test]
    fn status_column_keys() {
        let def = LogDef::parse(
            "x.def",
            "pattern = ^(?P<code>[0-9]+) (?P<message>.*)$\nhttp_status = CODE\nwin32_status = nothere\n",
            None,
        )
        .unwrap();
        let (_, s) = parse_lines(Arc::new(def), &["404 GET /x"]);
        assert_eq!(s.status.http.as_deref(), Some("CODE"));
        let st = s.status.resolve(&s.extra_names);
        assert_eq!(st.http, s.extra_names.iter().position(|n| n == "code"));
        assert!(st.http.is_some());
        assert_eq!((st.substatus, st.win32), (None, None));
        // Without the keys nothing is explained.
        let def = LogDef::parse("y.def", "pattern = ^(?P<message>.*)$\n", None).unwrap();
        assert!(def.status.is_empty());
    }

    /// A second `#Fields:` header with fewer fields adds no columns but removes none either:
    /// the table shows the columns of every part of the file.
    #[test]
    fn iis_w3c_fields_change_within_file() {
        let (e, s) = parse_lines(
            builtin("iisweb.log.def"),
            &[
                "#Fields: date time s-ip cs-method cs-uri-stem c-ip sc-status",
                "2026-10-23 08:00:01 10.0.0.5 GET /a 10.0.0.9 200",
                "#Fields: date time cs-method cs-uri-stem sc-status time-taken",
                "2026-10-23 09:00:01 POST /b 500 15",
            ],
        );
        let titles: Vec<String> = s
            .columns
            .iter()
            .map(|c| c.title.clone().unwrap_or_else(|| format!("{:?}", c.kind)))
            .collect();
        assert_eq!(
            titles,
            [
                "Role(DateTime)",
                "Status",
                "Client IP",
                "Request",
                "Server IP",
                "Time (ms)"
            ]
        );
        // Only the fields of the actual headers are extra columns (not the definition's defaults).
        assert_eq!(s.extra_names, ["s-ip", "c-ip", "sc-status", "time-taken"]);
        assert_eq!(
            (extra(&e[0], &s, "s-ip"), extra(&e[0], &s, "c-ip")),
            ("10.0.0.5", "10.0.0.9")
        );
        assert_eq!(extra(&e[0], &s, "time-taken"), "");
        assert_eq!(
            (e[1].message.as_str(), extra(&e[1], &s, "s-ip")),
            ("POST /b", "")
        );
        assert_eq!(
            (
                extra(&e[1], &s, "sc-status"),
                extra(&e[1], &s, "time-taken")
            ),
            ("500", "15")
        );
        assert_eq!(e[1].severity, Severity::Error);
    }

    /// Several definitions for the same files: the user's folder wins over the folder next to
    /// the program and over the built-in one, whatever the file names are.
    #[test]
    fn priority_of_matching_definitions() {
        let base = std::env::temp_dir().join(format!("rustrace_defprio_{}", std::process::id()));
        let (exe_dir, user_dir) = (base.join("exe"), base.join("user"));
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::create_dir_all(&user_dir).unwrap();
        let def = |name: &str| format!("name = {name}\nfiles = u_ex*.log\nformat = w3c\n");
        std::fs::write(exe_dir.join("iisweb.log.def"), def("next to exe")).unwrap(); // overrides built-in
        std::fs::write(exe_dir.join("aaa.def"), def("exe aaa")).unwrap();
        std::fs::write(user_dir.join("zzz.def"), def("user zzz")).unwrap();
        // An own copy with the same file name as a predefined one does not replace it.
        std::fs::write(user_dir.join("IISWEB.log.def"), def("own iisweb")).unwrap();
        let dirs = [exe_dir, user_dir];
        let set = load_from(&dirs, SourceFilter::All);
        assert!(set.errors.is_empty(), "{:?}", set.errors);
        let matching = matching_by_name(&set.defs, Path::new("u_ex261023.log"));
        let names: Vec<&str> = matching.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["own iisweb", "user zzz", "exe aaa", "next to exe"]);
        assert_eq!(
            by_file_name(&set.defs, Path::new("x/u_ex1.log"))
                .unwrap()
                .name,
            "own iisweb"
        );
        let prio: Vec<(&str, u8)> = set
            .defs
            .iter()
            .map(|d| (d.id.as_str(), d.priority))
            .collect();
        assert_eq!(
            prio,
            [
                ("IISWEB.log.def", 2),
                ("zzz.def", 2),
                ("aaa.def", 1),
                ("iisweb.log.def", 1),
                ("cbs.log.def", 0),
                ("dism.log.def", 0),
                ("exchconnect.log.def", 0)
            ]
        );
        assert!(set.defs[0].is_own() && !set.defs[0].same_as(&set.defs[3]));
        // Only the predefined or only the own definitions.
        let ids = |f| -> Vec<String> {
            load_from(&dirs, f)
                .defs
                .iter()
                .map(|d| d.name.clone())
                .collect()
        };
        assert_eq!(ids(SourceFilter::Own), ["own iisweb", "user zzz"]);
        assert_eq!(
            ids(SourceFilter::Predefined),
            [
                "exe aaa",
                "next to exe",
                "Windows CBS log",
                "DISM log",
                "Exchange Transport Connectivity log"
            ]
        );
        // The texts for the notice are there whatever the filter is.
        let set = load_from(&dirs, SourceFilter::Own);
        assert_eq!(
            set.predefined.keys().collect::<Vec<_>>(),
            [
                "aaa.def",
                "cbs.log.def",
                "dism.log.def",
                "exchconnect.log.def",
                "iisweb.log.def"
            ]
        );
        assert_eq!(
            set.own.keys().collect::<Vec<_>>(),
            ["iisweb.log.def", "zzz.def"]
        );
        std::fs::remove_dir_all(&base).ok();
    }

    /// The notice after an update: new, changed (key by key, with the own copy) and removed
    /// predefined definitions; unchanged ones and comment-only changes are not reported.
    #[test]
    fn updates_of_predefined_definitions() {
        let map = |items: &[(&str, &str)]| -> BTreeMap<String, String> {
            items
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect()
        };
        let known = map(&[
            (
                "iis.def",
                "name = IIS\nfiles = u_ex*.log\nrole.message = cs-method cs-uri-stem\ntitle.a = A",
            ),
            ("same.def", "name = Same\nfiles = s.log"),
            ("old.def", "name = Old one\nfiles = o.log"),
        ]);
        let predefined = map(&[
            (
                "iis.def",
                "# new comment\nname = IIS\nfiles = u_ex*.log; u_in*.log\nrole.message = cs-method cs-uri-stem\ntitle.b = B",
            ),
            (
                "same.def",
                "# only a comment differs\nname = Same\nfiles = s.log",
            ),
            ("new.def", "name = Brand new\nfiles = n.log"),
        ]);
        let own: BTreeMap<String, (PathBuf, String)> = [(
            "iis.def".to_owned(),
            (
                PathBuf::from("own/iis.def"),
                "name = IIS\nfiles = u_ex*.log\nrole.message = cs-uri-stem\ntitle.b = B".to_owned(),
            ),
        )]
        .into();
        let u = definition_updates(&known, &predefined, &own);
        let kinds: Vec<(&str, UpdateKind)> = u.iter().map(|d| (d.id.as_str(), d.kind)).collect();
        assert_eq!(
            kinds,
            [
                ("iis.def", UpdateKind::Changed),
                ("new.def", UpdateKind::New),
                ("old.def", UpdateKind::Removed)
            ]
        );
        assert_eq!(u[1].name, "Brand new");
        assert_eq!(u[2].name, "Old one");
        assert_eq!(u[0].own_path.as_deref(), Some(Path::new("own/iis.def")));
        let s = |v: &str| Some(v.to_owned());
        assert_eq!(
            u[0].changes,
            [
                KeyChange {
                    key: "files".into(),
                    old: s("u_ex*.log"),
                    new: s("u_ex*.log; u_in*.log"),
                    own: Some(s("u_ex*.log"))
                },
                KeyChange {
                    key: "title.a".into(),
                    old: s("A"),
                    new: None,
                    own: Some(None)
                },
                KeyChange {
                    key: "title.b".into(),
                    old: None,
                    new: s("B"),
                    own: Some(s("B"))
                },
            ]
        );
        assert!(definition_updates(&predefined, &predefined, &BTreeMap::new()).is_empty());
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
