//! Export of the parsed log as a table (#32): which rows, which columns and the value of every
//! cell, written by one of the writers (`csv`, …). Pure – no egui; the UI part is
//! `app/tabexport.rs` + `app/dialogs/tabexport.rs`.
//!
//! The rows are streamed from the shared `LogStore` (no copy of the log); the background thread
//! holds an `Arc` of it while the app keeps new read results waiting (like the background filter).

pub mod csv;
pub mod xlsx;
mod zip;

use crate::colfilter::DateLayout;
use crate::errcodes;
use crate::parser::{Encoding, RawReader, Severity};
use crate::store::{EntryRef, LogStore};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Which rows are exported.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Every entry in memory (all merged logs / rotated parts).
    All,
    /// The rows shown in the table (search filter, severity toggles, column filters, time range).
    #[default]
    Visible,
    /// The selected rows (#25).
    Selected,
    /// The bookmarked rows (#22).
    Bookmarked,
    /// Highlighted rows with their context – the plan of "Export highlighted rows" (blocks).
    Highlighted,
}

/// File format of the table export.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum TableFormat {
    #[default]
    Csv,
    Xlsx,
}

impl TableFormat {
    pub fn extension(self) -> &'static str {
        match self {
            TableFormat::Csv => "csv",
            TableFormat::Xlsx => "xlsx",
        }
    }
}

/// Settings of the table export (saved in the config).
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    pub scope: Scope,
    pub format: TableFormat,
    /// CSV separator; `None` = the list separator of Windows (`;` for SK / CS / DE).
    pub csv_separator: Option<char>,
    pub csv_encoding: csv::CsvEncoding,
    pub header: bool,
    /// Dates as ISO `2026-10-27 19:49:05.123` (sorts correctly) or in the format of the UI language.
    pub iso_dates: bool,
    /// Values starting with `=`, `+`, `-`, `@` get a leading `'` (CSV / formula injection).
    pub safe_for_excel: bool,
    /// The severity column in English instead of the UI language.
    pub severity_english: bool,
    /// Excel: row colors of the highlight rules and severities (light palette).
    pub xlsx_colors: bool,
    /// Chosen columns per log type (name of the log definition, "" = CMTrace): (key, on) in order.
    pub columns: BTreeMap<String, Vec<(String, bool)>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scope: Scope::Visible,
            format: TableFormat::Csv,
            csv_separator: None,
            csv_encoding: csv::CsvEncoding::Utf8Bom,
            header: true,
            iso_dates: true,
            safe_for_excel: true,
            severity_english: false,
            xlsx_colors: false,
            columns: BTreeMap::new(),
        }
    }
}

/// A column of the export.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Col {
    Line,
    /// File label (name of the log / rotated part).
    File,
    /// Full path of the source file.
    Path,
    DateTime,
    /// Time since the previous row (seconds).
    Delta,
    /// Severity as text (Info / Warning / Error).
    Severity,
    Component,
    Thread,
    Message,
    /// Extra column of a log definition (index into `Schema::extra_names`).
    Extra(usize),
    /// Name of the first matching highlight rule.
    Rule,
    /// Bookmark note.
    Note,
    /// Error codes found in the message with their description.
    Codes,
    /// The original text of the entry in its file.
    Raw,
    /// Number of the block (highlighted rows with context).
    Block,
}

impl Col {
    /// Columns that are not in the table (offered unticked).
    pub const ADDITIONAL: [Col; 7] = [
        Col::Severity,
        Col::Path,
        Col::Rule,
        Col::Note,
        Col::Codes,
        Col::Raw,
        Col::Block,
    ];

    /// Key in the saved column choice (extra columns by name, so the choice survives other
    /// `#Fields:` sets).
    pub fn key(self, extra_names: &[String]) -> String {
        match self {
            Col::Line => "line".into(),
            Col::File => "file".into(),
            Col::Path => "path".into(),
            Col::DateTime => "datetime".into(),
            Col::Delta => "delta".into(),
            Col::Severity => "severity".into(),
            Col::Component => "component".into(),
            Col::Thread => "thread".into(),
            Col::Message => "message".into(),
            Col::Extra(i) => format!(
                "extra:{}",
                extra_names.get(i).map(String::as_str).unwrap_or("")
            ),
            Col::Rule => "rule".into(),
            Col::Note => "note".into(),
            Col::Codes => "codes".into(),
            Col::Raw => "raw".into(),
            Col::Block => "block".into(),
        }
    }

    pub fn from_key(key: &str, extra_names: &[String]) -> Option<Col> {
        Some(match key {
            "line" => Col::Line,
            "file" => Col::File,
            "path" => Col::Path,
            "datetime" => Col::DateTime,
            "delta" => Col::Delta,
            "severity" => Col::Severity,
            "component" => Col::Component,
            "thread" => Col::Thread,
            "message" => Col::Message,
            "rule" => Col::Rule,
            "note" => Col::Note,
            "codes" => Col::Codes,
            "raw" => Col::Raw,
            "block" => Col::Block,
            _ => {
                let name = key.strip_prefix("extra:")?;
                Col::Extra(
                    extra_names
                        .iter()
                        .position(|n| n.eq_ignore_ascii_case(name))?,
                )
            }
        })
    }
}

/// The column choice shown in the dialog: the saved choice of the log type (its order, columns
/// that no longer exist dropped), then the columns it does not know yet – the table columns
/// (`table`, ticked) and every other extra / additional column (unticked).
pub fn column_choice(
    table: &[Col],
    extra_count: usize,
    extra_names: &[String],
    saved: Option<&[(String, bool)]>,
) -> Vec<(Col, bool)> {
    let mut out: Vec<(Col, bool)> = Vec::new();
    for (key, on) in saved.unwrap_or_default() {
        if let Some(c) = Col::from_key(key, extra_names)
            && !out.iter().any(|(x, _)| *x == c)
        {
            out.push((c, *on));
        }
    }
    let rest = table
        .iter()
        .map(|&c| (c, true))
        .chain((0..extra_count).map(|i| (Col::Extra(i), false)))
        .chain(Col::ADDITIONAL.iter().map(|&c| (c, false)));
    for (c, on) in rest {
        if !out.iter().any(|(x, _)| *x == c) {
            out.push((c, on));
        }
    }
    out
}

/// A column with its header text.
#[derive(Clone, Debug)]
pub struct ColumnSpec {
    pub col: Col,
    pub title: String,
}

/// The rows of the export (entry indexes in order).
#[derive(Clone, Debug)]
pub enum Rows {
    /// `0..n`.
    All(usize),
    List(Vec<usize>),
    /// Inclusive blocks (highlighted rows with context); the block number is a column.
    Blocks(Vec<(usize, usize)>),
}

impl Rows {
    pub fn len(&self) -> usize {
        match self {
            Rows::All(n) => *n,
            Rows::List(v) => v.len(),
            Rows::Blocks(b) => b.iter().map(|(a, b)| b - a + 1).sum(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// (entry index, block number from 1; 0 = no blocks).
    pub fn iter(&self) -> Box<dyn Iterator<Item = (usize, usize)> + '_> {
        match self {
            Rows::All(n) => Box::new((0..*n).map(|i| (i, 0))),
            Rows::List(v) => Box::new(v.iter().map(|&i| (i, 0))),
            Rows::Blocks(b) => Box::new(
                b.iter()
                    .enumerate()
                    .flat_map(|(n, &(a, z))| (a..=z).map(move |i| (i, n + 1))),
            ),
        }
    }
}

/// A file of the log.
#[derive(Clone, Debug)]
pub struct FileInfo {
    pub label: String,
    pub path: Option<PathBuf>,
    pub encoding: Encoding,
}

/// A highlight rule as the export needs it.
#[derive(Clone, Debug)]
pub struct RuleInfo {
    pub name: String,
    pub regex: Regex,
    /// "Whole row" scope (colors the row).
    pub row: bool,
    pub match_component: bool,
    pub fg: Option<[u8; 4]>,
    pub bg: Option<[u8; 4]>,
}

impl RuleInfo {
    fn matches(&self, e: &EntryRef) -> bool {
        self.regex.is_match(e.message)
            || (self.match_component && crate::filter::other_columns_match(&self.regex, e))
    }
}

/// What the Δ column is measured from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DeltaFrom {
    /// The previous exported row with a time.
    Previous,
    /// The previous row of the log with a time (whatever is exported).
    Log,
    /// A reference time (ms).
    Reference(i64),
}

/// Everything the cells are made of (moved into the export thread).
pub struct Context {
    pub store: Arc<LogStore>,
    pub files: Vec<FileInfo>,
    pub rules: Vec<RuleInfo>,
    /// (file id, line) → note.
    pub notes: HashMap<(u16, usize), String>,
    /// Severity texts: info, warning, error.
    pub severity_names: [String; 3],
    /// Date format of the UI language; `None` = ISO.
    pub date_layout: Option<DateLayout>,
    pub delta: DeltaFrom,
    /// (text, background) of warnings and errors (row colors without a rule).
    pub severity_colors: [Option<([u8; 4], [u8; 4])>; 2],
}

/// The value of a cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell<'a> {
    Empty,
    Text(Cow<'a, str>),
    Int(i64),
    /// A duration in seconds.
    Seconds(f64),
    /// A date/time (ms since the epoch, local time) with its display text.
    Date {
        ms: i64,
        text: String,
    },
}

impl Cell<'_> {
    /// Text of the cell; `decimal` = the decimal separator of numbers with a fraction.
    pub fn text(&self, decimal: char) -> Cow<'_, str> {
        match self {
            Cell::Empty => Cow::Borrowed(""),
            Cell::Text(t) => Cow::Borrowed(t.as_ref()),
            Cell::Int(i) => Cow::Owned(i.to_string()),
            Cell::Seconds(s) => {
                let t = format!("{s:.3}");
                Cow::Owned(if decimal == '.' {
                    t
                } else {
                    t.replace('.', &decimal.to_string())
                })
            }
            Cell::Date { text, .. } => Cow::Borrowed(text.as_str()),
        }
    }
}

/// Colors of a row (as in the table: the first matching "whole row" rule, else the severity).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct RowLook {
    pub fg: Option<[u8; 4]>,
    pub bg: Option<[u8; 4]>,
}

/// Receives the rows (one writer per file format).
pub trait TableSink {
    fn header(&mut self, cols: &[ColumnSpec]) -> io::Result<()>;
    fn row(&mut self, cells: &[Cell<'_>], look: RowLook) -> io::Result<()>;
}

/// Progress and cancel flag shared with the UI.
#[derive(Clone, Default)]
pub struct Control {
    pub done: Arc<AtomicUsize>,
    pub cancel: Arc<AtomicBool>,
}

impl Control {
    fn check(&self, done: usize) -> io::Result<()> {
        if done.is_multiple_of(1024) {
            self.done.store(done, Ordering::Relaxed);
            if self.cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
        }
        Ok(())
    }
}

/// How many rows back the previous dated row of the log is looked for (Δ from the log).
const MAX_LOOK_BACK: usize = 1000;

/// Builds the cells of the rows.
pub struct CellMaker<'a> {
    ctx: &'a Context,
    codes: HashMap<u32, String>,
    /// Open files for the raw lines (`None` = could not be opened).
    raw: HashMap<u16, Option<RawReader>>,
    prev_ts: Option<i64>,
}

impl<'a> CellMaker<'a> {
    pub fn new(ctx: &'a Context) -> Self {
        Self {
            ctx,
            codes: HashMap::new(),
            raw: HashMap::new(),
            prev_ts: None,
        }
    }

    fn severity_index(s: Severity) -> usize {
        match s {
            Severity::Info => 0,
            Severity::Warning => 1,
            Severity::Error => 2,
        }
    }

    /// The cells of entry `idx` (block number `block`) and the colors of its row.
    pub fn row(
        &mut self,
        cols: &[ColumnSpec],
        idx: usize,
        block: usize,
    ) -> (Vec<Cell<'a>>, RowLook) {
        let ctx = self.ctx;
        let store: &'a LogStore = &ctx.store;
        let e = store.at(idx);
        let ts = e.timestamp();
        let mut cells = Vec::with_capacity(cols.len());
        for c in cols {
            cells.push(match c.col {
                Col::Line => Cell::Int(e.line_no as i64),
                Col::File => text(
                    ctx.files
                        .get(e.file as usize)
                        .map_or("", |f| f.label.as_str()),
                ),
                Col::Path => ctx
                    .files
                    .get(e.file as usize)
                    .and_then(|f| f.path.as_ref())
                    .map_or(Cell::Empty, |p| {
                        Cell::Text(Cow::Owned(p.display().to_string()))
                    }),
                Col::DateTime => {
                    let iso = e.datetime();
                    match ts {
                        Some(ms) => Cell::Date {
                            ms,
                            text: match &ctx.date_layout {
                                Some(l) => l.format_iso(&iso),
                                None => iso,
                            },
                        },
                        None if iso.is_empty() => Cell::Empty,
                        None => Cell::Text(Cow::Owned(iso)),
                    }
                }
                Col::Delta => self.delta(store, idx, ts),
                Col::Severity => text(&ctx.severity_names[Self::severity_index(e.severity)]),
                Col::Component => text(e.component),
                Col::Thread => text(e.thread),
                Col::Message => text(e.message),
                Col::Extra(i) => text(e.extra(i)),
                Col::Rule => ctx
                    .rules
                    .iter()
                    .find(|r| r.matches(&e))
                    .map_or(Cell::Empty, |r| text(&r.name)),
                Col::Note => ctx
                    .notes
                    .get(&(e.file, e.line_no))
                    .map_or(Cell::Empty, |n| text(n)),
                Col::Codes => self.codes(e.message),
                Col::Raw => self.raw(&e),
                Col::Block => {
                    if block == 0 {
                        Cell::Empty
                    } else {
                        Cell::Int(block as i64)
                    }
                }
            });
        }
        if ts.is_some() {
            self.prev_ts = ts;
        }
        (cells, self.look(&e))
    }

    fn delta(&self, store: &LogStore, idx: usize, ts: Option<i64>) -> Cell<'a> {
        let Some(ts) = ts else {
            return Cell::Empty;
        };
        let prev = match self.ctx.delta {
            DeltaFrom::Previous => self.prev_ts,
            DeltaFrom::Log => (idx.saturating_sub(MAX_LOOK_BACK)..idx)
                .rev()
                .find_map(|i| store.timestamp(i)),
            DeltaFrom::Reference(r) => Some(r),
        };
        prev.map_or(Cell::Empty, |p| Cell::Seconds((ts - p) as f64 / 1000.0))
    }

    /// "0x80070005 – Access is denied.; …" for the codes in the message.
    fn codes(&mut self, message: &str) -> Cell<'a> {
        let found = errcodes::find_codes(message);
        if found.is_empty() {
            return Cell::Empty;
        }
        let parts: Vec<String> = found
            .into_iter()
            .map(|c| {
                self.codes
                    .entry(c)
                    .or_insert_with(|| {
                        let info = errcodes::lookup(c);
                        format!("{} – {}", info.hex(), info.short())
                    })
                    .clone()
            })
            .collect();
        Cell::Text(Cow::Owned(parts.join("; ")))
    }

    fn raw(&mut self, e: &EntryRef) -> Cell<'a> {
        let (Some(range), Some(f)) = (e.raw_range(), self.ctx.files.get(e.file as usize)) else {
            return Cell::Empty;
        };
        let reader = self
            .raw
            .entry(e.file)
            .or_insert_with(|| f.path.as_ref().and_then(|p| RawReader::open(p).ok()));
        match reader.as_mut().and_then(|r| r.read(range, f.encoding).ok()) {
            Some(t) => Cell::Text(Cow::Owned(t)),
            None => Cell::Empty,
        }
    }

    fn look(&self, e: &EntryRef) -> RowLook {
        if let Some(r) = self.ctx.rules.iter().find(|r| r.row && r.matches(e)) {
            return RowLook { fg: r.fg, bg: r.bg };
        }
        let pair = match e.severity {
            Severity::Warning => self.ctx.severity_colors[0],
            Severity::Error => self.ctx.severity_colors[1],
            Severity::Info => None,
        };
        pair.map_or_else(RowLook::default, |(fg, bg)| RowLook {
            fg: Some(fg),
            bg: Some(bg),
        })
    }
}

fn text(s: &str) -> Cell<'_> {
    if s.is_empty() {
        Cell::Empty
    } else {
        Cell::Text(Cow::Borrowed(s))
    }
}

/// Writes the header and every row into `sink`; returns the number of rows.
/// Cancelling (`ctl.cancel`) ends with an `Interrupted` error.
pub fn write_table(
    ctx: &Context,
    rows: &Rows,
    cols: &[ColumnSpec],
    header: bool,
    sink: &mut dyn TableSink,
    ctl: &Control,
) -> io::Result<usize> {
    if header {
        sink.header(cols)?;
    }
    let mut maker = CellMaker::new(ctx);
    let mut n = 0;
    for (idx, block) in rows.iter() {
        if idx >= ctx.store.len() {
            continue;
        }
        let (cells, look) = maker.row(cols, idx, block);
        sink.row(&cells, look)?;
        n += 1;
        ctl.check(n)?;
    }
    ctl.done.store(n, Ordering::Relaxed);
    Ok(n)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::parser::LogEntry;

    pub fn entry(line_no: usize, dt: &str, sev: Severity, comp: &str, msg: &str) -> LogEntry {
        LogEntry {
            line_no,
            message: msg.to_owned(),
            component: comp.to_owned(),
            datetime: dt.to_owned(),
            thread: "12".to_owned(),
            severity: sev,
            extra: Vec::new(),
            raw: (0, 0),
        }
    }

    pub fn context(store: LogStore) -> Context {
        Context {
            store: Arc::new(store),
            files: vec![FileInfo {
                label: "a.log".into(),
                path: Some(PathBuf::from(r"C:\Logs\a.log")),
                encoding: Encoding::Utf8,
            }],
            rules: vec![RuleInfo {
                name: "Download".into(),
                regex: Regex::new("(?i)download").unwrap(),
                row: true,
                match_component: false,
                fg: None,
                bg: Some([1, 2, 3, 255]),
            }],
            notes: [((0, 2), "check this".to_owned())].into(),
            severity_names: ["Info".into(), "Warning".into(), "Error".into()],
            date_layout: None,
            delta: DeltaFrom::Previous,
            severity_colors: [None, Some(([9, 9, 9, 255], [8, 8, 8, 255]))],
        }
    }

    pub fn sample() -> Context {
        context(LogStore::from_entries([
            entry(
                1,
                "2026-10-27 19:49:05.123",
                Severity::Info,
                "Comp",
                "Download started",
            ),
            entry(
                2,
                "2026-10-27 19:49:06.623",
                Severity::Error,
                "Comp",
                "Failed, error 0x80070005\nsecond line",
            ),
            entry(3, "", Severity::Warning, "", "=SUM(A1)"),
        ]))
    }

    pub fn specs(cols: &[Col]) -> Vec<ColumnSpec> {
        cols.iter()
            .map(|&col| ColumnSpec {
                col,
                title: format!("{col:?}"),
            })
            .collect()
    }

    /// Collects the rows as texts.
    struct Collect(Vec<Vec<String>>, Vec<RowLook>);

    impl TableSink for Collect {
        fn header(&mut self, cols: &[ColumnSpec]) -> io::Result<()> {
            self.0.push(cols.iter().map(|c| c.title.clone()).collect());
            Ok(())
        }
        fn row(&mut self, cells: &[Cell<'_>], look: RowLook) -> io::Result<()> {
            self.0
                .push(cells.iter().map(|c| c.text('.').into_owned()).collect());
            self.1.push(look);
            Ok(())
        }
    }

    #[test]
    fn rows_of_the_scopes() {
        assert_eq!(
            Rows::All(3).iter().collect::<Vec<_>>(),
            [(0, 0), (1, 0), (2, 0)]
        );
        assert_eq!(Rows::List(vec![2, 5]).len(), 2);
        let b = Rows::Blocks(vec![(1, 2), (6, 6)]);
        assert_eq!(b.len(), 3);
        assert_eq!(b.iter().collect::<Vec<_>>(), [(1, 1), (2, 1), (6, 2)]);
        assert!(Rows::List(Vec::new()).is_empty());
    }

    #[test]
    fn cells_of_all_columns() {
        let ctx = sample();
        let cols = specs(&[
            Col::Line,
            Col::File,
            Col::Path,
            Col::DateTime,
            Col::Delta,
            Col::Severity,
            Col::Component,
            Col::Message,
            Col::Rule,
            Col::Note,
            Col::Codes,
            Col::Block,
        ]);
        let mut out = Collect(Vec::new(), Vec::new());
        let n = write_table(
            &ctx,
            &Rows::Blocks(vec![(0, 2)]),
            &cols,
            true,
            &mut out,
            &Control::default(),
        )
        .unwrap();
        assert_eq!(n, 3);
        assert_eq!(out.0[0][0], "Line");
        assert_eq!(
            out.0[1],
            [
                "1",
                "a.log",
                r"C:\Logs\a.log",
                "2026-10-27 19:49:05.123",
                "",
                "Info",
                "Comp",
                "Download started",
                "Download",
                "",
                "",
                "1"
            ]
        );
        assert_eq!(out.0[2][4], "1.500");
        assert_eq!(out.0[2][5], "Error");
        assert_eq!(out.0[2][9], "check this");
        assert!(
            out.0[2][10].starts_with("0x80070005 – "),
            "{}",
            out.0[2][10]
        );
        // Row colors: the rule, the error color, nothing for a warning without colors.
        assert_eq!(out.1[0].bg, Some([1, 2, 3, 255]));
        assert_eq!(out.1[1].fg, Some([9, 9, 9, 255]));
        assert_eq!(out.1[2], RowLook::default());
        // Δ from the log / from a reference.
        let mut ctx = sample();
        ctx.delta = DeltaFrom::Reference(ctx.store.timestamp(0).unwrap() - 2000);
        let mut m = CellMaker::new(&ctx);
        assert_eq!(m.row(&specs(&[Col::Delta]), 1, 0).0, [Cell::Seconds(3.5)]);
    }

    #[test]
    fn column_choice_and_keys() {
        let names = vec!["sc-status".to_owned(), "c-ip".to_owned()];
        assert_eq!(Col::Extra(1).key(&names), "extra:c-ip");
        assert_eq!(
            Col::from_key("extra:SC-STATUS", &names),
            Some(Col::Extra(0))
        );
        assert_eq!(Col::from_key("extra:gone", &names), None);
        let table = [Col::Line, Col::DateTime, Col::Extra(0), Col::Message];
        let fresh = column_choice(&table, 2, &names, None);
        assert_eq!(&fresh[..4], &table.map(|c| (c, true)));
        assert_eq!(fresh[4], (Col::Extra(1), false));
        assert_eq!(fresh.len(), 5 + Col::ADDITIONAL.len());
        // A saved order wins; unknown keys are dropped, new columns are appended.
        let saved = vec![
            ("message".to_owned(), true),
            ("line".to_owned(), false),
            ("extra:gone".to_owned(), true),
            ("codes".to_owned(), true),
        ];
        let c = column_choice(&table, 2, &names, Some(&saved));
        assert_eq!(
            &c[..4],
            [
                (Col::Message, true),
                (Col::Line, false),
                (Col::Codes, true),
                (Col::DateTime, true)
            ]
        );
    }

    #[test]
    fn cancel_stops_the_export() {
        let ctx = context(LogStore::from_entries(
            (0..5000).map(|i| entry(i + 1, "", Severity::Info, "", "x")),
        ));
        let ctl = Control::default();
        ctl.cancel.store(true, Ordering::Relaxed);
        let mut out = Collect(Vec::new(), Vec::new());
        let r = write_table(
            &ctx,
            &Rows::All(5000),
            &specs(&[Col::Line]),
            false,
            &mut out,
            &ctl,
        );
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(out.0.len(), 1024);
    }
}
