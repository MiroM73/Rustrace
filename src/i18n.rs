//! Localization.
//!
//! All user-visible text is looked up by key, e.g. `t!("menu.file")`.
//!
//! Language files are plain UTF-8 text files named `<code>.lang` (e.g. `sk.lang`, `pt-BR.lang`):
//!
//! ```text
//! # comment
//! language.name = Slovenčina
//! status.entries = Entries: {count}
//! ```
//!
//! * one `key = value` pair per line, `#` starts a comment line,
//! * `\n` inside a value is a line break, `\\` is a backslash,
//! * `{name}` is a placeholder filled in by the program.
//!
//! English, Slovak, Czech and German are built into the executable. More languages can be
//! added **without recompiling** by dropping a `<code>.lang` file into one of these folders:
//!
//! 1. `lang/` next to the executable,
//! 2. `<config dir>/RusTrace/lang/` (Windows: `%APPDATA%\RusTrace\lang`).
//!
//! An external file with the same code as a built-in language overrides only the keys it
//! contains, so a translation can also be fixed without recompiling. Missing keys fall back
//! to English, and a key missing even in English is shown as the key itself.
//!
//! Error-code descriptions use keys `code.0xXXXXXXXX`, so new codes can be added the same way.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

/// Built-in language files (code, content).
const BUILTIN: &[(&str, &str)] = &[
    ("en", include_str!("../lang/en.lang")),
    ("sk", include_str!("../lang/sk.lang")),
    ("cs", include_str!("../lang/cs.lang")),
    ("de", include_str!("../lang/de.lang")),
];

/// Translates a key, optionally filling `{name}` placeholders:
/// `t!("status.entries", count = 42)`.
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::tr($key)
    };
    ($key:expr, $($name:ident = $val:expr),+ $(,)?) => {{
        let mut s = $crate::i18n::tr($key);
        $( s = s.replace(concat!("{", stringify!($name), "}"), &($val).to_string()); )+
        s
    }};
}

#[derive(Clone, Debug)]
pub struct LangInfo {
    pub code: String,
    pub name: String,
    /// True when the language comes only from an external file.
    pub external: bool,
}

struct Catalog {
    english: HashMap<String, String>,
    current: HashMap<String, String>,
    code: String,
    available: Vec<LangInfo>,
}

fn catalog() -> &'static RwLock<Catalog> {
    static CAT: OnceLock<RwLock<Catalog>> = OnceLock::new();
    CAT.get_or_init(|| {
        let english = load_language("en");
        RwLock::new(Catalog {
            current: english.clone(),
            english,
            code: "en".into(),
            available: scan_available(),
        })
    })
}

/// Parses the content of a language file.
pub fn parse(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        if k.is_empty() {
            continue;
        }
        map.insert(k.to_owned(), unescape(v.trim()));
    }
    map
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Folders searched for external language files, in order of increasing priority.
pub fn lang_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("lang")))
    {
        dirs.push(d);
    }
    if let Some(d) = user_lang_dir() {
        dirs.push(d);
    }
    dirs
}

/// Per-user language folder (`%APPDATA%\RusTrace\lang` on Windows).
pub fn user_lang_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("RusTrace").join("lang"))
}

fn external_files(code: &str) -> Vec<PathBuf> {
    lang_dirs()
        .into_iter()
        .map(|d| d.join(format!("{code}.lang")))
        .filter(|p| p.is_file())
        .collect()
}

/// Built-in strings of a language merged with external overrides.
fn load_language(code: &str) -> HashMap<String, String> {
    let mut map = BUILTIN
        .iter()
        .find(|(c, _)| c.eq_ignore_ascii_case(code))
        .map(|(_, text)| parse(text))
        .unwrap_or_default();
    for f in external_files(code) {
        if let Ok(text) = std::fs::read_to_string(&f) {
            map.extend(parse(&text));
        }
    }
    map
}

fn scan_available() -> Vec<LangInfo> {
    let mut list: Vec<LangInfo> = BUILTIN
        .iter()
        .map(|(code, _)| LangInfo {
            code: (*code).to_owned(),
            name: String::new(),
            external: false,
        })
        .collect();
    for dir in lang_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("lang") {
                continue;
            }
            let Some(code) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !list.iter().any(|l| l.code.eq_ignore_ascii_case(code)) {
                list.push(LangInfo {
                    code: code.to_owned(),
                    name: String::new(),
                    external: true,
                });
            }
        }
    }
    for l in &mut list {
        l.name = load_language(&l.code)
            .remove("language.name")
            .unwrap_or_else(|| l.code.clone());
    }
    list.sort_by_key(|a| a.code.to_lowercase());
    list
}

/// Language of the operating system, e.g. "sk" or "de-AT".
pub fn system_language() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en".into())
}

/// Picks the best available code for a requested one ("de-AT" → "de-AT" or "de" or "en").
fn resolve(requested: &str, available: &[LangInfo]) -> String {
    let req = requested.replace('_', "-");
    let req = req.split('.').next().unwrap_or("en"); // "sk_SK.UTF-8" on Linux
    if let Some(l) = available.iter().find(|l| l.code.eq_ignore_ascii_case(req)) {
        return l.code.clone();
    }
    let base = req.split('-').next().unwrap_or(req);
    if let Some(l) = available.iter().find(|l| l.code.eq_ignore_ascii_case(base)) {
        return l.code.clone();
    }
    "en".into()
}

/// Switches the language. An empty code means "same as the operating system".
/// Returns the code actually used.
pub fn set_language(code: &str) -> String {
    let mut cat = catalog().write().unwrap_or_else(|e| e.into_inner());
    let requested = if code.is_empty() {
        system_language()
    } else {
        code.to_owned()
    };
    let resolved = resolve(&requested, &cat.available);
    cat.english = load_language("en");
    cat.current = load_language(&resolved);
    cat.code = resolved.clone();
    resolved
}

/// Re-reads the language folders (new files, edited translations).
pub fn reload(code: &str) -> String {
    {
        let mut cat = catalog().write().unwrap_or_else(|e| e.into_inner());
        cat.available = scan_available();
    }
    set_language(code)
}

pub fn available() -> Vec<LangInfo> {
    catalog()
        .read()
        .map(|c| c.available.clone())
        .unwrap_or_default()
}

/// Translation of a key, or `None` when no language defines it.
pub fn tr_opt(key: &str) -> Option<String> {
    let cat = catalog().read().ok()?;
    cat.current
        .get(key)
        .or_else(|| cat.english.get(key))
        .cloned()
}

/// Translation of a key (falls back to English, then to the key itself).
pub fn tr(key: &str) -> String {
    tr_opt(key).unwrap_or_else(|| key.to_owned())
}

/// The English language file – a starting point for a new translation.
pub fn template() -> &'static str {
    BUILTIN[0].1
}

/// Writes the template as a new language file and returns its path.
pub fn export_template(dir: &Path, code: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{code}.lang"));
    if path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            path.display().to_string(),
        ));
    }
    std::fs::write(&path, template())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn placeholders(s: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = s;
        while let Some(i) = rest.find('{') {
            let Some(j) = rest[i..].find('}') else { break };
            out.insert(rest[i + 1..i + j].to_owned());
            rest = &rest[i + j + 1..];
        }
        out
    }

    #[test]
    fn parse_file_format() {
        let m = parse("# c\n\u{feff}a = Hello {name}\nb=x \\n y = z\n bad line\n");
        assert_eq!(m["a"], "Hello {name}");
        assert_eq!(m["b"], "x \n y = z");
        assert_eq!(m.len(), 2);
    }

    /// Every built-in language must contain exactly the English keys with the same placeholders.
    #[test]
    fn builtin_languages_are_complete() {
        let en = parse(BUILTIN[0].1);
        for (code, text) in &BUILTIN[1..] {
            let m = parse(text);
            for (k, v) in &en {
                let tv = m
                    .get(k)
                    .unwrap_or_else(|| panic!("{code}: missing key {k}"));
                assert_eq!(
                    placeholders(v),
                    placeholders(tv),
                    "{code}: placeholders differ in {k}"
                );
            }
            for k in m.keys() {
                assert!(en.contains_key(k), "{code}: unknown key {k}");
            }
        }
    }

    #[test]
    fn resolve_codes() {
        let av = vec![
            LangInfo {
                code: "en".into(),
                name: String::new(),
                external: false,
            },
            LangInfo {
                code: "de".into(),
                name: String::new(),
                external: false,
            },
            LangInfo {
                code: "pt-BR".into(),
                name: String::new(),
                external: true,
            },
        ];
        assert_eq!(resolve("de-AT", &av), "de");
        assert_eq!(resolve("pt_BR.UTF-8", &av), "pt-BR");
        assert_eq!(resolve("fr-FR", &av), "en");
    }
}
