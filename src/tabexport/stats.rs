//! Summary and Statistics sheets of the Excel export (#32, part 3) and the whole `.xlsx` job:
//! data sheets (optionally one per source file), counts collected while the rows are written.

use super::xlsx::{XlsxOptions, XlsxWriter};
use super::{Cell, ColumnSpec, Context, Control, Rows, write_table_stats};
use crate::errcodes;
use crate::parser::Severity;
use crate::store::EntryRef;
use std::collections::{BTreeMap, HashMap};
use std::io::{self, Seek, Write};

/// Hours of the "entries per hour" table (ms).
const HOUR: i64 = 3_600_000;
/// Rows of the "top error codes" table.
pub const TOP_CODES: usize = 20;

/// Counts over the exported rows.
#[derive(Default, Debug)]
pub struct Stats {
    pub rows: usize,
    pub errors: usize,
    pub warnings: usize,
    pub first: Option<i64>,
    pub last: Option<i64>,
    /// Component → (errors, warnings).
    pub components: BTreeMap<String, (usize, usize)>,
    /// Codes found in error / warning messages → count.
    pub codes: HashMap<u32, usize>,
    /// Start of the hour (ms) → (entries, errors, warnings).
    pub hours: BTreeMap<i64, (usize, usize, usize)>,
}

impl Stats {
    pub fn add(&mut self, e: &EntryRef) {
        self.rows += 1;
        let (err, warn) = match e.severity {
            Severity::Error => (1, 0),
            Severity::Warning => (0, 1),
            Severity::Info => (0, 0),
        };
        self.errors += err;
        self.warnings += warn;
        if err + warn > 0 {
            let c = self.components.entry(e.component.to_owned()).or_default();
            c.0 += err;
            c.1 += warn;
            for code in errcodes::find_codes(e.message) {
                *self.codes.entry(code).or_default() += 1;
            }
        }
        if let Some(ts) = e.timestamp() {
            self.first = Some(self.first.map_or(ts, |f| f.min(ts)));
            self.last = Some(self.last.map_or(ts, |l| l.max(ts)));
            let h = self.hours.entry(ts.div_euclid(HOUR) * HOUR).or_default();
            h.0 += 1;
            h.1 += err;
            h.2 += warn;
        }
    }

    /// Components with errors or warnings, most errors first.
    pub fn components_sorted(&self) -> Vec<(&str, usize, usize)> {
        let mut v: Vec<(&str, usize, usize)> = self
            .components
            .iter()
            .map(|(c, &(e, w))| (c.as_str(), e, w))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(b.0)));
        v
    }

    /// The most frequent codes (count desc, then code).
    pub fn top_codes(&self, n: usize) -> Vec<(u32, usize)> {
        let mut v: Vec<(u32, usize)> = self.codes.iter().map(|(&c, &n)| (c, n)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(n);
        v
    }
}

/// Translated texts of the Summary and Statistics sheets (made on the UI thread).
#[derive(Clone, Debug, Default)]
pub struct SheetTexts {
    pub summary_name: String,
    pub statistics_name: String,
    /// Rows known before the export (version, files, log type, filters …).
    pub summary: Vec<(String, String)>,
    pub rows: String,
    pub errors: String,
    pub warnings: String,
    pub first: String,
    pub last: String,
    pub sheets: String,
    pub truncated: String,
    pub per_component: String,
    pub component: String,
    pub top_codes: String,
    pub code: String,
    pub count: String,
    pub description: String,
    pub per_hour: String,
    pub hour: String,
    pub entries: String,
}

/// What an `.xlsx` export writes besides the rows.
#[derive(Clone, Debug, Default)]
pub struct XlsxExtras {
    pub summary: bool,
    pub statistics: bool,
    /// One data sheet per source file (merged logs / rotated parts).
    pub sheet_per_file: bool,
    pub texts: SheetTexts,
}

fn txt(s: &str) -> Cell<'static> {
    Cell::Text(s.to_owned().into())
}

fn num(n: usize) -> Cell<'static> {
    Cell::Int(n as i64)
}

/// Writes the whole workbook; returns (output, rows, cut cells).
#[allow(clippy::too_many_arguments)]
pub fn write_xlsx<W: Write + Seek>(
    out: W,
    ctx: &Context,
    rows: &Rows,
    cols: &[ColumnSpec],
    header: bool,
    opts: XlsxOptions,
    extras: &XlsxExtras,
    ctl: &Control,
) -> io::Result<(W, usize, usize)> {
    let mut w = XlsxWriter::new(out, opts);
    let mut stats = Stats::default();
    let files: Vec<u16> = if extras.sheet_per_file && ctx.files.len() > 1 {
        let mut seen: Vec<u16> = Vec::new();
        for (i, _) in rows.iter() {
            let f = ctx.store.file(i);
            if !seen.contains(&f) {
                seen.push(f);
            }
        }
        seen
    } else {
        Vec::new()
    };
    let mut n = 0;
    if files.len() > 1 {
        for f in files {
            let part = rows.of_file(&ctx.store, f);
            let label = ctx.files.get(f as usize).map_or("", |x| x.label.as_str());
            w.new_data_sheet(label)?;
            n += write_table_stats(ctx, &part, cols, header, &mut w, ctl, Some(&mut stats), n)?;
        }
    } else {
        n = write_table_stats(ctx, rows, cols, header, &mut w, ctl, Some(&mut stats), 0)?;
    }
    let t = &extras.texts;
    if extras.statistics {
        let (cells, bold) = statistics_rows(&stats, t);
        w.small_sheet(
            &t.statistics_name,
            &[40.0, 12.0, 12.0, 70.0],
            &cells,
            &bold,
            Some(1),
        )?;
    }
    if extras.summary {
        let mut cells: Vec<Vec<Cell>> = t
            .summary
            .iter()
            .map(|(k, v)| vec![txt(k), txt(v)])
            .collect();
        let date = |ms: Option<i64>| {
            ms.map_or(Cell::Empty, |ms| Cell::Date {
                ms,
                text: String::new(),
            })
        };
        cells.push(vec![txt(&t.rows), num(stats.rows)]);
        cells.push(vec![txt(&t.errors), num(stats.errors)]);
        cells.push(vec![txt(&t.warnings), num(stats.warnings)]);
        cells.push(vec![txt(&t.first), date(stats.first)]);
        cells.push(vec![txt(&t.last), date(stats.last)]);
        cells.push(vec![txt(&t.sheets), num(w.data_sheets())]);
        if w.truncated > 0 {
            cells.push(vec![txt(&t.truncated), num(w.truncated)]);
        }
        w.small_sheet(&t.summary_name, &[34.0, 90.0], &cells, &[], Some(0))?;
    }
    let cut = w.truncated;
    Ok((w.finish()?, n, cut))
}

/// The Statistics sheet: errors / warnings per component, top error codes, entries per hour.
fn statistics_rows(s: &Stats, t: &SheetTexts) -> (Vec<Vec<Cell<'static>>>, Vec<usize>) {
    let mut rows: Vec<Vec<Cell<'static>>> = Vec::new();
    let mut bold = Vec::new();
    let mut title = |rows: &mut Vec<Vec<Cell<'static>>>, cells: Vec<Cell<'static>>| {
        bold.push(rows.len());
        rows.push(cells);
    };
    title(&mut rows, vec![txt(&t.per_component)]);
    title(
        &mut rows,
        vec![txt(&t.component), txt(&t.errors), txt(&t.warnings)],
    );
    for (c, e, w) in s.components_sorted() {
        rows.push(vec![txt(c), num(e), num(w)]);
    }
    rows.push(Vec::new());
    title(&mut rows, vec![txt(&t.top_codes)]);
    title(
        &mut rows,
        vec![
            txt(&t.code),
            txt(&t.count),
            Cell::Empty,
            txt(&t.description),
        ],
    );
    for (code, n) in s.top_codes(TOP_CODES) {
        let info = errcodes::lookup(code);
        rows.push(vec![
            txt(&info.hex()),
            num(n),
            Cell::Empty,
            txt(&info.short()),
        ]);
    }
    rows.push(Vec::new());
    title(&mut rows, vec![txt(&t.per_hour)]);
    title(
        &mut rows,
        vec![
            txt(&t.hour),
            txt(&t.entries),
            txt(&t.errors),
            txt(&t.warnings),
        ],
    );
    for (&h, &(n, e, w)) in &s.hours {
        rows.push(vec![
            Cell::Date {
                ms: h,
                text: String::new(),
            },
            num(n),
            num(e),
            num(w),
        ]);
    }
    (rows, bold)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, specs};
    use super::super::*;
    use super::*;
    use crate::store::LogStore;

    fn texts() -> XlsxExtras {
        XlsxExtras {
            summary: true,
            statistics: true,
            sheet_per_file: false,
            texts: SheetTexts {
                summary_name: "Summary".into(),
                statistics_name: "Statistics".into(),
                summary: vec![("Version".into(), "1.0".into())],
                rows: "Rows".into(),
                errors: "Errors".into(),
                warnings: "Warnings".into(),
                component: "Component".into(),
                per_component: "Per component".into(),
                top_codes: "Top codes".into(),
                per_hour: "Per hour".into(),
                ..Default::default()
            },
        }
    }

    fn store() -> LogStore {
        let mut s = LogStore::new();
        s.push(
            entry(
                1,
                "2026-10-27 08:10:00.000",
                Severity::Error,
                "A",
                "x 0x80070005",
            ),
            0,
        );
        s.push(
            entry(
                2,
                "2026-10-27 08:20:00.000",
                Severity::Error,
                "A",
                "y 0x80070005",
            ),
            0,
        );
        s.push(
            entry(1, "2026-10-27 09:05:00.000", Severity::Warning, "B", "z"),
            1,
        );
        s.push(entry(2, "", Severity::Info, "B", "no time"), 1);
        s
    }

    #[test]
    fn counts() {
        let st = store();
        let mut s = Stats::default();
        for i in 0..st.len() {
            s.add(&st.at(i));
        }
        assert_eq!((s.rows, s.errors, s.warnings), (4, 2, 1));
        assert_eq!(s.components_sorted(), [("A", 2, 0), ("B", 0, 1)]);
        assert_eq!(s.top_codes(5), [(0x8007_0005, 2)]);
        let hours: Vec<(usize, usize, usize)> = s.hours.values().copied().collect();
        assert_eq!(hours, [(2, 2, 0), (1, 0, 1)]);
        assert_eq!(s.first, crate::store::iso_to_ms("2026-10-27 08:10:00.000"));
        assert_eq!(s.last, crate::store::iso_to_ms("2026-10-27 09:05:00.000"));
    }

    #[test]
    fn workbook_with_summary_statistics_and_sheet_per_file() {
        let mut ctx = super::super::tests::context(store());
        ctx.files.push(FileInfo {
            label: "b.log".into(),
            path: None,
            encoding: crate::parser::Encoding::Utf8,
        });
        let mut extras = texts();
        extras.sheet_per_file = true;
        let (out, rows, cut) = write_xlsx(
            io::Cursor::new(Vec::new()),
            &ctx,
            &Rows::All(4),
            &specs(&[Col::Line, Col::Message]),
            true,
            XlsxOptions::default(),
            &extras,
            &Control::default(),
        )
        .unwrap();
        assert_eq!((rows, cut), (4, 0));
        let files: HashMap<String, String> = crate::update::unzip(&out.into_inner())
            .unwrap()
            .into_iter()
            .map(|(n, b)| (n, String::from_utf8(b).unwrap()))
            .collect();
        let wb = &files["xl/workbook.xml"];
        let pos = |n: &str| wb.find(&format!("name=\"{n}\"")).unwrap();
        assert!(pos("Summary") < pos("Statistics"));
        assert!(pos("Statistics") < pos("a.log"));
        assert!(pos("a.log") < pos("b.log"));
        let sheet_of = |name: &str| {
            let i = wb.find(&format!("name=\"{name}\" sheetId=\"")).unwrap();
            let id: String = wb[i + name.len() + 17..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            files[&format!("xl/worksheets/sheet{id}.xml")].clone()
        };
        let a = sheet_of("a.log");
        assert!(a.contains("x 0x80070005") && !a.contains("no time"));
        assert!(sheet_of("b.log").contains("no time"));
        let summary = sheet_of("Summary");
        assert!(summary.contains(">Version<") && summary.contains(">Rows<"));
        assert!(summary.contains("<v>4</v>"));
        let stats = sheet_of("Statistics");
        assert!(stats.contains(">Per component<") && stats.contains(">0x80070005<"));
    }
}
