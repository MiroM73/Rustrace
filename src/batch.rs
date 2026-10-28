//! `rustrace FILE... --export <out.csv|xlsx|jsonl|html>` (#32, part 4, with #26): reads the
//! FILEs (merged by time like in the window), applies the view options of the command line
//! (`--search` / `--filter`, `--errors` / `--warnings`, `--from` / `--to`) and writes the
//! table export without opening a window – for scripted reports. The columns and options are
//! the saved ones of Export table (the table columns of the log type when none were saved).

use crate::cli::ViewOptions;
use crate::config::Config;
use crate::filter::{FilterOutput, FilterSpec};
use crate::logdef::{self, ColumnKind, Role, Schema, SourceFilter};
use crate::merge::{LogSet, ReadResult};
use crate::parser::Progress;
use crate::store::LogStore;
use crate::t;
use crate::tabexport::{self, Col, ColumnSpec, Control, DeltaFrom, Rows, TableFormat};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Reads the whole log set once.
fn read(files: &[PathBuf], cfg: &Config) -> Result<(LogStore, LogSet), String> {
    let mut set = LogSet::new(files, cfg.keyword_severity);
    set.defs = Arc::new(logdef::load_all(SourceFilter::from_key(&cfg.logdef_sources)).defs);
    set.merge_rotated = cfg.merge_rotated;
    let progress = Progress::default();
    let mut store = LogStore::new();
    // The first read returns everything; a second one only picks up what was appended meanwhile.
    for _ in 0..2 {
        match set.poll(&progress).map_err(|e| e.to_string())? {
            ReadResult::Reset {
                committed,
                provisional,
            } => {
                store = committed;
                store.append(provisional);
            }
            ReadResult::Appended { .. } | ReadResult::Unchanged => break,
        }
        if set.has_read() {
            break;
        }
    }
    if store.is_empty() && !set.errors().is_empty() {
        return Err(set.errors().join("\n"));
    }
    Ok((store, set))
}

/// Header text of a column (the same as in the window).
fn title(col: Col, schema: &Schema) -> String {
    let kind = match col {
        Col::Line => return t!("column.line"),
        Col::File => return t!("column.file"),
        Col::Path => return t!("tabexport.col.path"),
        Col::Delta => return t!("tabexport.col.delta"),
        Col::Severity => return t!("tabexport.col.severity"),
        Col::Rule => return t!("tabexport.col.rule"),
        Col::Note => return t!("tabexport.col.note"),
        Col::Codes => return t!("tabexport.col.codes"),
        Col::Raw => return t!("tabexport.col.raw"),
        Col::Block => return t!("tabexport.col.block"),
        Col::Message => ColumnKind::Role(Role::Message),
        Col::Component => ColumnKind::Role(Role::Component),
        Col::DateTime => ColumnKind::Role(Role::DateTime),
        Col::Thread => ColumnKind::Role(Role::Thread),
        Col::Extra(i) => ColumnKind::Extra(i),
    };
    if let Some(t) = schema
        .columns
        .iter()
        .find(|c| c.kind == kind)
        .and_then(|c| c.title.clone())
    {
        return t;
    }
    match kind {
        ColumnKind::Role(Role::Message) => t!("column.message"),
        ColumnKind::Role(Role::Component) => t!("column.component"),
        ColumnKind::Role(Role::DateTime) => t!("column.datetime"),
        ColumnKind::Role(Role::Thread) => t!("column.thread"),
        ColumnKind::Extra(i) => schema.extra_names.get(i).cloned().unwrap_or_default(),
        _ => String::new(),
    }
}

/// Writes the export; returns the message for the console.
pub fn export(
    files: &[PathBuf],
    view: &ViewOptions,
    target: &Path,
    cfg: &Config,
) -> Result<String, String> {
    let format = TableFormat::from_path(target)
        .ok_or_else(|| t!("cli.error.export_format", file = target.display()))?;
    let (store, set) = read(files, cfg)?;
    let schema = set.schema().clone();
    let date_pattern = t!("format.date");
    let bound = |text: &Option<String>, end| {
        text.as_deref()
            .and_then(|t| crate::export::parse_range_bound(t, &date_pattern, end))
    };
    let search = match &view.search {
        Some(s) => {
            let p = if view.regex {
                s.clone()
            } else {
                regex::escape(s)
            };
            Some(
                regex::RegexBuilder::new(&p)
                    .case_insensitive(!view.case)
                    .size_limit(20 << 20)
                    .build()
                    .map_err(|e| e.to_string())?,
            )
        }
        None => None,
    };
    let spec = FilterSpec {
        search,
        filter_mode: view.filter,
        show_sev: view.severity.map_or([true; 3], |s| s.show_sev()),
        time_range: (bound(&view.from, false), bound(&view.to, true)),
        ..Default::default()
    };
    let mut out = FilterOutput::default();
    spec.run(&store, 0..store.len(), &mut out);

    let s = &cfg.table_export;
    let merged = set.files().len() > 1;
    let table: Vec<Col> = std::iter::once(Col::Line)
        .chain(merged.then_some(Col::File))
        .chain(schema.columns.iter().map(|c| match c.kind {
            ColumnKind::Role(Role::Message) => Col::Message,
            ColumnKind::Role(Role::Component) => Col::Component,
            ColumnKind::Role(Role::DateTime) => Col::DateTime,
            ColumnKind::Role(Role::Thread) => Col::Thread,
            ColumnKind::Extra(i) => Col::Extra(i),
            ColumnKind::File => Col::File,
            ColumnKind::Delta => Col::Delta,
        }))
        .collect();
    let choice = tabexport::column_choice(
        &table,
        schema.extra_names.len(),
        &schema.extra_names,
        s.columns.get(&schema.name).map(Vec::as_slice),
    );
    let cols: Vec<ColumnSpec> = choice
        .into_iter()
        .filter(|(_, on)| *on)
        .map(|(col, _)| ColumnSpec {
            col,
            title: title(col, &schema),
        })
        .collect();
    let rules = cfg
        .rules
        .iter()
        .filter(|r| r.enabled)
        .filter_map(|r| {
            let p = if r.is_regex {
                r.pattern.clone()
            } else {
                regex::escape(&r.pattern)
            };
            let regex = regex::RegexBuilder::new(&p)
                .case_insensitive(!r.case_sensitive)
                .build()
                .ok()?;
            let (fg, bg) = r.colors(false);
            Some(tabexport::RuleInfo {
                name: r.name.clone(),
                regex,
                row: r.scope == crate::config::HighlightScope::Row,
                match_component: r.match_component,
                fg: r.use_fg.then_some(fg),
                bg: r.use_bg.then_some(bg),
            })
        })
        .collect();
    let rows = Rows::List(out.filtered);
    let count = rows.len();
    let format_name = if schema.name.is_empty() {
        t!("format.builtin")
    } else {
        schema.name.clone()
    };
    let mut info = vec![
        (
            t!("tabexport.sum.version"),
            format!("RusTrace {}", env!("CARGO_PKG_VERSION")),
        ),
        (
            t!("tabexport.sum.time"),
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        ),
        (t!("tabexport.sum.format"), format_name),
    ];
    for f in set.files() {
        let p = f
            .path
            .as_ref()
            .map_or(f.name.clone(), |p| p.display().to_string());
        info.push((t!("tabexport.sum.file"), p));
    }
    info.push((t!("tabexport.sum.entries"), store.len().to_string()));
    info.push((
        t!("tabexport.sum.scope"),
        t!("tabexport.scope.visible", count = count),
    ));
    if let Some(s) = &view.search {
        info.push((t!("tabexport.sum.search"), s.clone()));
    }
    if view.from.is_some() || view.to.is_some() {
        info.push((
            t!("tabexport.sum.time_range"),
            format!(
                "{} – {}",
                view.from.as_deref().unwrap_or(""),
                view.to.as_deref().unwrap_or("")
            ),
        ));
    }
    let palette = cfg.palette(false);
    let html_info = info.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    let job = tabexport::Job {
        ctx: tabexport::Context {
            store: Arc::new(store),
            files: set
                .files()
                .iter()
                .map(|f| tabexport::FileInfo {
                    label: f.name.clone(),
                    path: f.path.clone(),
                    encoding: f.encoding,
                })
                .collect(),
            rules,
            notes: Default::default(),
            severity_names: if s.severity_english {
                ["Info".into(), "Warning".into(), "Error".into()]
            } else {
                [
                    t!("severity.info"),
                    t!("severity.warning"),
                    t!("severity.error"),
                ]
            },
            date_layout: (!s.iso_dates).then(|| crate::colfilter::DateLayout::new(&date_pattern)),
            delta: DeltaFrom::Previous,
            severity_colors: if cfg.color_by_severity {
                [Some(palette.warning), Some(palette.error)]
            } else {
                [None, None]
            },
        },
        json_keys: cols
            .iter()
            .map(|c| c.col.json_key(&schema.extra_names))
            .collect(),
        cols,
        rows,
        format,
        header: s.header,
        csv: tabexport::csv::CsvOptions::new(
            s.csv_separator
                .unwrap_or_else(tabexport::csv::system_list_separator),
            s.csv_encoding,
            s.safe_for_excel,
        ),
        xlsx: tabexport::xlsx::XlsxOptions {
            colors: s.xlsx_colors,
            sheet_name: t!("tabexport.sheet.log"),
            ..Default::default()
        },
        extras: tabexport::stats::XlsxExtras {
            summary: s.xlsx_summary,
            statistics: s.xlsx_statistics,
            sheet_per_file: s.xlsx_sheet_per_file,
            texts: tabexport::stats::SheetTexts {
                summary_name: t!("tabexport.sheet.summary"),
                statistics_name: t!("tabexport.sheet.statistics"),
                summary: info,
                rows: t!("tabexport.sum.rows"),
                errors: t!("severity.error.plural"),
                warnings: t!("severity.warning.plural"),
                first: t!("tabexport.sum.first"),
                last: t!("tabexport.sum.last"),
                sheets: t!("tabexport.sum.sheets"),
                truncated: t!("tabexport.sum.truncated"),
                per_component: t!("tabexport.stat.per_component"),
                component: t!("column.component"),
                top_codes: t!(
                    "tabexport.stat.top_codes",
                    count = tabexport::stats::TOP_CODES
                ),
                code: t!("tabexport.stat.code"),
                count: t!("tabexport.stat.count"),
                description: t!("tabexport.stat.description"),
                per_hour: t!("tabexport.stat.per_hour"),
                hour: t!("tabexport.stat.hour"),
                entries: t!("tabexport.stat.entries"),
            },
        },
        html_title: files
            .first()
            .and_then(|p| p.file_name())
            .map_or_else(|| "RusTrace".into(), |n| n.to_string_lossy().into_owned()),
        html_info,
        html_colors: s.xlsx_colors,
    };
    let (n, _) = tabexport::run(&job, target, &Control::default()).map_err(|e| e.to_string())?;
    Ok(t!("tabexport.done", rows = n, path = target.display()))
}

/// Lets a program without a console (the release build) print to the console it was started
/// from (`cmd`, PowerShell); does nothing when there is none.
pub fn attach_console() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn AttachConsole(process: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        // SAFETY: no pointers; failing (no parent console) is fine.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A CMTrace log exported with `--errors` as CSV and as JSON Lines.
    #[test]
    fn export_from_the_command_line() {
        let dir = std::env::temp_dir().join(format!("rustrace_batch_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("app.log");
        let line = |msg: &str, t: u8, time: &str| {
            format!(
                "<![LOG[{msg}]LOG]!><time=\"{time}.000+000\" date=\"10-27-2026\" component=\"App\" context=\"\" type=\"{t}\" thread=\"1\" file=\"\">\r\n"
            )
        };
        let text = [
            line("Started", 1, "08:00:00"),
            line("Failed with 0x80070005", 3, "08:00:01"),
            line("Retrying", 2, "08:00:02"),
            line("Failed again", 3, "09:30:00"),
        ]
        .concat();
        std::fs::write(&log, text).unwrap();
        let mut cfg = Config::default();
        cfg.table_export.csv_separator = Some(',');
        let view = ViewOptions {
            severity: Some(crate::cli::Severities::Errors),
            to: Some("2026-10-27 09:00".into()),
            ..Default::default()
        };
        let csv = dir.join("out.csv");
        let msg = export(std::slice::from_ref(&log), &view, &csv, &cfg).unwrap();
        assert!(msg.contains('1'), "{msg}");
        let out = std::fs::read_to_string(&csv).unwrap();
        let lines: Vec<&str> = out.trim_start_matches('\u{feff}').lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[1].starts_with("2,Failed with 0x80070005,App,2026-10-27 08:00:01.000"));
        let jsonl = dir.join("out.jsonl");
        export(
            std::slice::from_ref(&log),
            &ViewOptions::default(),
            &jsonl,
            &cfg,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&jsonl).unwrap().lines().count(), 4);
        assert!(export(&[log], &view, &dir.join("out.pdf"), &cfg).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
