//! Command-line arguments (#26): `rustrace [OPTIONS] [FILE]...`.
//!
//! [`parse`] is a small hand-written parser (no dependency) that only turns the arguments into
//! [`Options`]; [`check_lang`] and [`validate`] check the values that need the language files
//! (language codes, the date order of `--from` / `--to`). Messages go through `t!()`, the help
//! and errors are shown with [`show_message`] (a message box on Windows, where the program has no
//! console, and stdout / stderr).
//!
//! Compatibility: a plain `rustrace.exe "C:\path\file.log"` (file association) is just a FILE, and
//! the internal `--restore <file>` of the update restart (#15) keeps its old form, so an older
//! installed version that installs this one still restarts it correctly.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::t;

/// The option of the update restart (hidden in the help; `update::RESTORE_ARG`).
use crate::update::RESTORE_ARG;

/// Severity filter of `--errors` / `--warnings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severities {
    /// Only errors.
    Errors,
    /// Errors and warnings.
    Warnings,
}

impl Severities {
    /// The info / warning / error toggles of the toolbar.
    pub fn show_sev(self) -> [bool; 3] {
        match self {
            Severities::Errors => [false, false, true],
            Severities::Warnings => [false, true, true],
        }
    }
}

/// What the opened log shows: search, filters, time range, selected line, following the end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewOptions {
    /// Line number (1-based) in the first FILE to select and scroll to.
    pub line: Option<usize>,
    /// Search text (non-empty).
    pub search: Option<String>,
    pub regex: bool,
    pub case: bool,
    /// Filter mode: only the rows matching the search.
    pub filter: bool,
    pub severity: Option<Severities>,
    /// Time range texts as entered (ISO or the date order of the UI language).
    pub from: Option<String>,
    pub to: Option<String>,
    /// `--tail` = `Some(true)`, `--no-follow` = `Some(false)`, none = the saved setting.
    pub follow: Option<bool>,
}

/// The parsed command line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub files: Vec<PathBuf>,
    pub view: ViewOptions,
    /// `--merge` was given (several FILEs are merged into one view anyway).
    pub merge: bool,
    /// UI language for this run only (not saved).
    pub lang: Option<String>,
    pub help: bool,
    pub version: bool,
    /// State file of the previous version after an update (`--restore <file>`).
    pub restore: Option<PathBuf>,
    /// Write the table export to this file and exit without a window (#32).
    pub export: Option<PathBuf>,
}

/// An invalid command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    UnknownOption(String),
    MissingValue(String),
    UnexpectedValue(String),
    InvalidLine(String),
    InvalidLanguage { value: String, available: String },
    InvalidTime { option: String, value: String },
    InvalidRegex(String),
    NeedsSearch(String),
    NeedsFile(String),
}

impl CliError {
    /// The translated message, with a hint to `--help`.
    pub fn message(&self) -> String {
        let msg = match self {
            CliError::UnknownOption(o) => t!("cli.error.unknown", option = o),
            CliError::MissingValue(o) => t!("cli.error.missing_value", option = o),
            CliError::UnexpectedValue(o) => t!("cli.error.unexpected_value", option = o),
            CliError::InvalidLine(v) => t!("cli.error.line", value = v),
            CliError::InvalidLanguage { value, available } => {
                t!("cli.error.lang", value = value, languages = available)
            }
            CliError::InvalidTime { option, value } => {
                t!("cli.error.time", option = option, value = value)
            }
            CliError::InvalidRegex(e) => t!("cli.error.regex", error = e),
            CliError::NeedsSearch(o) => t!("cli.error.needs_search", option = o),
            CliError::NeedsFile(o) => t!("cli.error.needs_file", option = o),
        };
        format!("{msg}\n\n{}", t!("cli.error.hint"))
    }
}

/// Parses the arguments (without the program name). Options may come before or after the
/// FILEs, values as `--line 5` or `--line=5`, `--` ends the options. A repeated option: the
/// last one counts.
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Options, CliError> {
    let mut o = Options::default();
    let mut args = args.into_iter();
    let mut only_files = false;
    while let Some(arg) = args.next() {
        if only_files {
            o.files.push(PathBuf::from(arg));
            continue;
        }
        let text = arg.to_string_lossy();
        let is_option = (text.len() > 1 && text.starts_with('-')) || text == "/?";
        if !is_option {
            o.files.push(PathBuf::from(arg));
            continue;
        }
        if text == "--" {
            only_files = true;
            continue;
        }
        let (name, inline) = match text.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_owned(), Some(v.to_owned())),
            _ => (text.into_owned(), None),
        };
        let takes_value = matches!(
            name.as_str(),
            "--line" | "--search" | "--from" | "--to" | "--lang" | "--export"
        ) || name == RESTORE_ARG;
        let value = if takes_value {
            match inline {
                Some(v) => Some(v),
                None => Some(
                    args.next()
                        .map(|v| v.to_string_lossy().into_owned())
                        .ok_or_else(|| CliError::MissingValue(name.clone()))?,
                ),
            }
        } else if inline.is_some() {
            return Err(CliError::UnexpectedValue(name));
        } else {
            None
        };
        let value = value.unwrap_or_default();
        match name.as_str() {
            "--line" => {
                o.view.line = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .filter(|&n| n >= 1)
                        .ok_or(CliError::InvalidLine(value))?,
                );
            }
            "--search" => o.view.search = (!value.is_empty()).then_some(value),
            "--regex" => o.view.regex = true,
            "--case" => o.view.case = true,
            "--filter" => o.view.filter = true,
            "--errors" => o.view.severity = Some(Severities::Errors),
            "--warnings" => o.view.severity = Some(Severities::Warnings),
            "--from" => o.view.from = Some(value),
            "--to" => o.view.to = Some(value),
            "--tail" => o.view.follow = Some(true),
            "--no-follow" => o.view.follow = Some(false),
            "--merge" => o.merge = true,
            "--lang" => o.lang = Some(value),
            "--export" => o.export = Some(PathBuf::from(value)),
            "--help" | "-h" | "-?" | "/?" => o.help = true,
            "--version" | "-V" => o.version = true,
            n if n == RESTORE_ARG => o.restore = Some(PathBuf::from(value)),
            _ => return Err(CliError::UnknownOption(name)),
        }
    }
    if o.view.search.is_none() {
        for (set, name) in [
            (o.view.regex, "--regex"),
            (o.view.case, "--case"),
            (o.view.filter, "--filter"),
        ] {
            if set {
                return Err(CliError::NeedsSearch(name.into()));
            }
        }
    }
    if o.view.line.is_some() && o.files.is_empty() && o.restore.is_none() {
        return Err(CliError::NeedsFile("--line".into()));
    }
    if o.export.is_some() && o.files.is_empty() && o.restore.is_none() {
        return Err(CliError::NeedsFile("--export".into()));
    }
    Ok(o)
}

/// The language code of `--lang` as the language files name it (case-insensitive match).
pub fn check_lang(value: &str, available: &[String]) -> Result<String, CliError> {
    available
        .iter()
        .find(|c| c.eq_ignore_ascii_case(value.trim()))
        .cloned()
        .ok_or_else(|| CliError::InvalidLanguage {
            value: value.to_owned(),
            available: available.join(", "),
        })
}

/// Checks the time range (`date_pattern` = `format.date` of the UI language) and the regular
/// expression of the search.
pub fn validate(view: &ViewOptions, date_pattern: &str) -> Result<(), CliError> {
    for (text, option, end) in [(&view.from, "--from", false), (&view.to, "--to", true)] {
        if let Some(text) = text
            && crate::export::parse_range_bound(text, date_pattern, end).is_none()
        {
            return Err(CliError::InvalidTime {
                option: option.into(),
                value: text.clone(),
            });
        }
    }
    if let Some(s) = &view.search
        && view.regex
        && let Err(e) = regex::RegexBuilder::new(s)
            .case_insensitive(!view.case)
            .size_limit(20 << 20)
            .build()
    {
        return Err(CliError::InvalidRegex(e.to_string()));
    }
    Ok(())
}

/// The help text (`--help`); `languages` = the codes `--lang` accepts. `tabs`: the option and
/// its description are separated by a tab (message box) instead of padding (console).
pub fn help_text(languages: &str, tabs: bool) -> String {
    let meta = |key: &str| format!("<{}>", t!(key));
    let opts: [(String, String); 17] = [
        ("--line <N>".into(), t!("cli.opt.line")),
        (
            format!("--search {}", meta("cli.meta.text")),
            t!("cli.opt.search"),
        ),
        ("--regex".into(), t!("cli.opt.regex")),
        ("--case".into(), t!("cli.opt.case")),
        ("--filter".into(), t!("cli.opt.filter")),
        ("--errors".into(), t!("cli.opt.errors")),
        ("--warnings".into(), t!("cli.opt.warnings")),
        (
            format!("--from {}", meta("cli.meta.time")),
            t!("cli.opt.from"),
        ),
        (format!("--to {}", meta("cli.meta.time")), t!("cli.opt.to")),
        ("--tail".into(), t!("cli.opt.tail")),
        ("--no-follow".into(), t!("cli.opt.no_follow")),
        ("--merge".into(), t!("cli.opt.merge")),
        (
            format!("--export {}", meta("cli.meta.file")),
            t!("cli.opt.export"),
        ),
        (
            format!("--lang {}", meta("cli.meta.code")),
            t!("cli.opt.lang", languages = languages),
        ),
        ("--help".into(), t!("cli.opt.help")),
        ("--version".into(), t!("cli.opt.version")),
        ("--".into(), t!("cli.opt.end")),
    ];
    let width = opts
        .iter()
        .map(|(o, _)| o.chars().count())
        .max()
        .unwrap_or(0);
    let mut s = format!("{}\n\n{}\n\n", t!("cli.usage"), t!("cli.files"));
    for (opt, desc) in opts {
        if tabs {
            s.push_str(&format!("{opt}\t{desc}\n"));
        } else {
            s.push_str(&format!("  {opt:width$}  {desc}\n"));
        }
    }
    s.push('\n');
    s.push_str(&t!("cli.time"));
    s.push_str("\n\n");
    s.push_str(&t!("cli.example"));
    s.push_str("\nrustrace C:\\Windows\\CCM\\Logs\\AppEnforce.log --search 0x87D00324 --filter");
    s.push_str(
        "\nrustrace AppEnforce.log AppDiscovery.log --errors --from \"2026-10-27 08:00\" --export errors.xlsx",
    );
    s
}

/// Shows a message: `console` on stdout / stderr (`error`) and, on Windows (the release build
/// has no console), `boxed` in a message box.
pub fn show_message(title: &str, console: &str, boxed: &str, error: bool) {
    if error {
        eprintln!("{console}");
    } else {
        println!("{console}");
    }
    #[cfg(windows)]
    message_box(title, boxed, error);
    #[cfg(not(windows))]
    let _ = (title, boxed);
}

#[cfg(windows)]
fn message_box(title: &str, text: &str, error: bool) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    const MB_ICONERROR: u32 = 0x10;
    const MB_ICONINFORMATION: u32 = 0x40;
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (text, title) = (wide(text), wide(title));
    let icon = if error {
        MB_ICONERROR
    } else {
        MB_ICONINFORMATION
    };
    // SAFETY: both strings are NUL-terminated UTF-16 and live until the call returns.
    unsafe {
        MessageBoxW(0, text.as_ptr(), title.as_ptr(), icon);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Options, CliError> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn file_association_and_no_arguments() {
        assert_eq!(p(&[]).unwrap(), Options::default());
        let o = p(&[r"C:\Windows\CCM\Logs\App Enforce.log"]).unwrap();
        assert_eq!(
            o.files,
            vec![PathBuf::from(r"C:\Windows\CCM\Logs\App Enforce.log")]
        );
        assert_eq!(o.view, ViewOptions::default());
        // A lone "-" and paths with "=" are files, not options.
        let o = p(&["-", r"C:\a=b.log"]).unwrap();
        assert_eq!(o.files.len(), 2);
    }

    #[test]
    fn issue_example() {
        let o = p(&[
            r"C:\Windows\CCM\Logs\AppEnforce.log",
            "--search",
            "0x87D00324",
            "--filter",
        ])
        .unwrap();
        assert_eq!(o.files.len(), 1);
        assert_eq!(o.view.search.as_deref(), Some("0x87D00324"));
        assert!(o.view.filter && !o.view.regex && !o.view.case);
    }

    #[test]
    fn all_options() {
        let o = p(&[
            "--line=12",
            "--search",
            "fail.*code",
            "--regex",
            "--case",
            "--filter",
            "--warnings",
            "--from",
            "2026-10-27 10:00",
            "--to=2026-10-27",
            "--no-follow",
            "--merge",
            "--lang",
            "de",
            "a.log",
            "b.log",
        ])
        .unwrap();
        assert_eq!(
            o.files,
            vec![PathBuf::from("a.log"), PathBuf::from("b.log")]
        );
        assert_eq!(
            o.view,
            ViewOptions {
                line: Some(12),
                search: Some("fail.*code".into()),
                regex: true,
                case: true,
                filter: true,
                severity: Some(Severities::Warnings),
                from: Some("2026-10-27 10:00".into()),
                to: Some("2026-10-27".into()),
                follow: Some(false),
            }
        );
        assert!(o.merge);
        assert_eq!(o.lang.as_deref(), Some("de"));
        assert_eq!(Severities::Errors.show_sev(), [false, false, true]);
        assert_eq!(Severities::Warnings.show_sev(), [false, true, true]);
    }

    #[test]
    fn last_option_wins_and_double_dash() {
        let o = p(&["--tail", "--no-follow", "--tail", "--errors", "x.log"]).unwrap();
        assert_eq!(o.view.follow, Some(true));
        assert_eq!(o.view.severity, Some(Severities::Errors));
        let o = p(&["--", "--line", "-x.log"]).unwrap();
        assert_eq!(
            o.files,
            vec![PathBuf::from("--line"), PathBuf::from("-x.log")]
        );
        // A search text that looks like an option is a value.
        let o = p(&["--search", "--help", "a.log"]).unwrap();
        assert_eq!(o.view.search.as_deref(), Some("--help"));
        assert!(!o.help);
    }

    #[test]
    fn help_version_restore() {
        for h in ["--help", "-h", "-?", "/?"] {
            assert!(p(&[h]).unwrap().help, "{h}");
        }
        assert!(p(&["--version"]).unwrap().version);
        assert!(p(&["-V"]).unwrap().version);
        // The update restart of older and current versions: `--restore <file>`.
        let o = p(&["--restore", r"C:\Temp\rustrace-restore-1.json"]).unwrap();
        assert_eq!(
            o.restore,
            Some(PathBuf::from(r"C:\Temp\rustrace-restore-1.json"))
        );
        assert!(o.files.is_empty());
    }

    #[test]
    fn errors() {
        assert_eq!(
            p(&["--bogus"]),
            Err(CliError::UnknownOption("--bogus".into()))
        );
        assert_eq!(p(&["-x"]), Err(CliError::UnknownOption("-x".into())));
        assert_eq!(
            p(&["a.log", "--line"]),
            Err(CliError::MissingValue("--line".into()))
        );
        assert_eq!(
            p(&["--restore"]),
            Err(CliError::MissingValue("--restore".into()))
        );
        assert_eq!(
            p(&["--filter=yes", "a.log"]),
            Err(CliError::UnexpectedValue("--filter".into()))
        );
        // --export (#32) needs a FILE and takes the target as its value.
        assert_eq!(
            p(&["a.log", "--errors", "--export=out.xlsx"])
                .unwrap()
                .export,
            Some(PathBuf::from("out.xlsx"))
        );
        assert_eq!(
            p(&["--export", "out.csv"]),
            Err(CliError::NeedsFile("--export".into()))
        );
        assert_eq!(
            p(&["a.log", "--export"]),
            Err(CliError::MissingValue("--export".into()))
        );
        for bad in ["0", "-3", "abc", ""] {
            assert_eq!(
                p(&["a.log", "--line", bad]),
                Err(CliError::InvalidLine(bad.into())),
                "{bad}"
            );
        }
        assert_eq!(
            p(&["--line", "5"]),
            Err(CliError::NeedsFile("--line".into()))
        );
        assert_eq!(
            p(&["a.log", "--filter"]),
            Err(CliError::NeedsSearch("--filter".into()))
        );
        assert_eq!(
            p(&["a.log", "--search", "", "--regex"]),
            Err(CliError::NeedsSearch("--regex".into()))
        );
        // Every error has a translated message.
        let msg = CliError::UnknownOption("--bogus".into()).message();
        assert!(
            msg.contains("--bogus") && !msg.contains("cli.error"),
            "{msg}"
        );
    }

    #[test]
    fn language_and_values() {
        let langs = vec!["cs".to_owned(), "de".into(), "en".into(), "sk".into()];
        assert_eq!(check_lang("SK", &langs), Ok("sk".into()));
        assert!(matches!(
            check_lang("xx", &langs),
            Err(CliError::InvalidLanguage { .. })
        ));
        let view = |from: &str, to: &str, search: &str, regex: bool| ViewOptions {
            from: (!from.is_empty()).then(|| from.into()),
            to: (!to.is_empty()).then(|| to.into()),
            search: (!search.is_empty()).then(|| search.into()),
            regex,
            ..Default::default()
        };
        let sk = "{d}.{m}.{y}";
        // ISO always, the language's date order too.
        assert_eq!(
            validate(&view("2026-10-27 10:00", "2026-10-27", "", false), sk),
            Ok(())
        );
        assert_eq!(
            validate(&view("27.10.2026 10:00:05", "", "", false), sk),
            Ok(())
        );
        assert!(matches!(
            validate(&view("yesterday", "", "", false), sk),
            Err(CliError::InvalidTime { .. })
        ));
        assert!(matches!(
            validate(&view("", "2026-13-40", "", false), sk),
            Err(CliError::InvalidTime { .. })
        ));
        assert_eq!(validate(&view("", "", "a(b", false), sk), Ok(()));
        assert!(matches!(
            validate(&view("", "", "a(b", true), sk),
            Err(CliError::InvalidRegex(_))
        ));
        let help = help_text("cs, de, en, sk", false);
        assert!(help.contains("--search <") && !help.contains("--restore"));
        assert!(!help.contains("cli.opt"), "{help}");
    }
}
