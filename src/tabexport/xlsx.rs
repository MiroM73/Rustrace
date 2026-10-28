//! Minimal Excel (.xlsx) writer of the table export (#32, part 2): typed cells (dates, numbers,
//! text), bold frozen header with AutoFilter, column widths, split over sheets at the row limit,
//! cell truncation at the Excel limit, optional row colors. Streams the worksheet XML into a
//! ZIP (`zip.rs`); styles and the workbook are written at the end.

use super::zip::ZipWriter;
use super::{Cell, Col, ColumnSpec, RowLook, TableSink};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, Seek, Write};

/// Rows of an Excel worksheet.
pub const MAX_ROWS: usize = 1_048_576;
/// Characters of an Excel cell.
pub const MAX_CELL_CHARS: usize = 32_767;

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

#[derive(Clone, Debug)]
pub struct XlsxOptions {
    /// Row colors of the highlight rules and severities.
    pub colors: bool,
    /// Rows per sheet including the header (`MAX_ROWS`; smaller in tests).
    pub max_rows: usize,
    /// Name of the first data sheet ("Log"); more sheets are "Log (2)" …
    pub sheet_name: String,
}

impl Default for XlsxOptions {
    fn default() -> Self {
        Self {
            colors: false,
            max_rows: MAX_ROWS,
            sheet_name: "Log".into(),
        }
    }
}

/// Number format of a cell style.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Kind {
    General,
    Date,
    Seconds,
    Header,
}

/// (number format, text color, background color) of a cell style.
type StyleKey = (Kind, Option<[u8; 3]>, Option<[u8; 3]>);

#[derive(Default)]
struct Styles {
    /// (bold, color)
    fonts: Vec<(bool, Option<[u8; 3]>)>,
    fills: Vec<Option<[u8; 3]>>,
    /// (kind, font, fill)
    xfs: Vec<(Kind, usize, usize)>,
    map: HashMap<StyleKey, usize>,
}

impl Styles {
    fn new() -> Self {
        let mut s = Styles {
            fonts: vec![(false, None), (true, None)],
            // The first two fills are reserved by Excel.
            fills: vec![None, None],
            ..Default::default()
        };
        s.get(Kind::General, None, None);
        s
    }

    /// Index of the cell style (created on first use).
    fn get(&mut self, kind: Kind, fg: Option<[u8; 3]>, bg: Option<[u8; 3]>) -> usize {
        if let Some(&i) = self.map.get(&(kind, fg, bg)) {
            return i;
        }
        let bold = kind == Kind::Header;
        let font = match self.fonts.iter().position(|&f| f == (bold, fg)) {
            Some(i) => i,
            None => {
                self.fonts.push((bold, fg));
                self.fonts.len() - 1
            }
        };
        let fill = match bg {
            None => 0,
            Some(c) => match self.fills.iter().position(|&f| f == Some(c)) {
                Some(i) => i,
                None => {
                    self.fills.push(Some(c));
                    self.fills.len() - 1
                }
            },
        };
        self.xfs.push((kind, font, fill));
        let i = self.xfs.len() - 1;
        self.map.insert((kind, fg, bg), i);
        i
    }

    fn xml(&self) -> String {
        let rgb = |c: [u8; 3]| format!("FF{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
        let mut s = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="{NS_MAIN}"><numFmts count="2"><numFmt numFmtId="164" formatCode="yyyy\-mm\-dd\ hh:mm:ss.000"/><numFmt numFmtId="165" formatCode="0.000"/></numFmts>"#
        );
        let _ = write!(s, r#"<fonts count="{}">"#, self.fonts.len());
        for (bold, color) in &self.fonts {
            s.push_str("<font>");
            if *bold {
                s.push_str("<b/>");
            }
            s.push_str(r#"<sz val="11"/>"#);
            if let Some(c) = color {
                let _ = write!(s, r#"<color rgb="{}"/>"#, rgb(*c));
            }
            s.push_str(r#"<name val="Calibri"/><family val="2"/></font>"#);
        }
        let _ = write!(
            s,
            r#"</fonts><fills count="{}"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>"#,
            self.fills.len()
        );
        for c in self.fills.iter().skip(2).flatten() {
            let _ = write!(
                s,
                r#"<fill><patternFill patternType="solid"><fgColor rgb="{}"/><bgColor indexed="64"/></patternFill></fill>"#,
                rgb(*c)
            );
        }
        let _ = write!(
            s,
            r#"</fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="{}">"#,
            self.xfs.len()
        );
        for (kind, font, fill) in &self.xfs {
            let fmt = match kind {
                Kind::Date => 164,
                Kind::Seconds => 165,
                Kind::General | Kind::Header => 0,
            };
            let _ = write!(
                s,
                r#"<xf numFmtId="{fmt}" fontId="{font}" fillId="{fill}" borderId="0" xfId="0" applyNumberFormat="1" applyFont="1" applyFill="1"/>"#
            );
        }
        s.push_str(r#"</cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#);
        s
    }
}

/// A worksheet of the workbook.
struct Sheet {
    name: String,
    /// Number of the `sheetN.xml` file.
    file: usize,
    /// Shown before the data sheets (Summary, Statistics).
    front: bool,
    /// Range of the AutoFilter (`A1:H100`).
    filter: Option<String>,
}

pub struct XlsxWriter<W: Write + Seek> {
    zip: ZipWriter<W>,
    opts: XlsxOptions,
    styles: Styles,
    sheets: Vec<Sheet>,
    /// Columns and whether the header row is written.
    cols: Vec<ColumnSpec>,
    header: bool,
    /// A data sheet is open; rows written into it (header included).
    open: bool,
    rows: usize,
    /// Base name of the data sheets and how many of them there are.
    base: String,
    parts: usize,
    /// Cells cut at `MAX_CELL_CHARS`.
    pub truncated: usize,
    buf: String,
}

/// Column letters of a 0-based index (A, B, …, Z, AA, …).
pub fn col_name(mut i: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (i % 26) as u8);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// Excel date serial (days since 1899-12-30) of a local time in ms since 1970.
pub fn excel_date(ms: i64) -> f64 {
    ms as f64 / 86_400_000.0 + 25_569.0
}

/// A valid sheet name: no `[]:*?/\`, at most 31 characters.
pub fn sheet_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if "[]:*?/\\".contains(c) { '_' } else { c })
        .take(31)
        .collect();
    if s.trim().is_empty() {
        "Sheet".into()
    } else {
        s
    }
}

/// XML text: escaped, characters XML 1.0 does not allow removed.
fn push_xml(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
}

fn width(col: Col) -> f32 {
    match col {
        Col::Line | Col::Block => 8.0,
        Col::DateTime => 23.0,
        Col::Delta | Col::Severity => 11.0,
        Col::Message | Col::Raw => 100.0,
        Col::Path | Col::Codes => 45.0,
        Col::Note => 30.0,
        Col::File | Col::Component | Col::Rule => 20.0,
        Col::Thread | Col::Extra(_) => 16.0,
    }
}

fn rgb(c: Option<[u8; 4]>) -> Option<[u8; 3]> {
    c.map(|c| [c[0], c[1], c[2]])
}

impl<W: Write + Seek> XlsxWriter<W> {
    pub fn new(out: W, opts: XlsxOptions) -> Self {
        let base = sheet_name(&opts.sheet_name);
        Self {
            zip: ZipWriter::new(out),
            opts,
            styles: Styles::new(),
            sheets: Vec::new(),
            cols: Vec::new(),
            header: false,
            open: false,
            rows: 0,
            base,
            parts: 0,
            truncated: 0,
            buf: String::new(),
        }
    }

    /// Number of data sheets written so far.
    #[cfg_attr(not(test), allow(dead_code))] // used by the Summary / Statistics sheets
    pub fn data_sheets(&self) -> usize {
        self.sheets.iter().filter(|s| !s.front).count()
    }

    /// The next data rows go to a new sheet with this name (one sheet per source file).
    #[allow(dead_code)] // used by the sheet per source file
    pub fn new_data_sheet(&mut self, name: &str) -> io::Result<()> {
        self.end_sheet()?;
        self.base = sheet_name(name);
        self.parts = 0;
        Ok(())
    }

    fn unique_name(&self, name: String) -> String {
        let taken = |n: &str| self.sheets.iter().any(|s| s.name.eq_ignore_ascii_case(n));
        if !taken(&name) {
            return name;
        }
        (2..)
            .map(|i| {
                let suffix = format!(" ({i})");
                let keep = 31 - suffix.len();
                format!("{}{suffix}", name.chars().take(keep).collect::<String>())
            })
            .find(|n| !taken(n))
            .unwrap()
    }

    fn start_sheet(
        &mut self,
        name: String,
        front: bool,
        cols: &[(Col, f32)],
        freeze: bool,
    ) -> io::Result<()> {
        let name = self.unique_name(name);
        let file = self.sheets.len() + 1;
        self.sheets.push(Sheet {
            name,
            file,
            front,
            filter: None,
        });
        self.zip
            .start_file(&format!("xl/worksheets/sheet{file}.xml"))?;
        let mut s = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="{NS_MAIN}" xmlns:r="{NS_REL}"><sheetViews><sheetView workbookViewId="0">"#
        );
        if freeze {
            s.push_str(r#"<pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/><selection pane="bottomLeft" activeCell="A2" sqref="A2"/>"#);
        }
        s.push_str(r#"</sheetView></sheetViews><sheetFormatPr defaultRowHeight="15"/>"#);
        if !cols.is_empty() {
            s.push_str("<cols>");
            for (i, (_, w)) in cols.iter().enumerate() {
                let _ = write!(
                    s,
                    r#"<col min="{n}" max="{n}" width="{w}" customWidth="1"/>"#,
                    n = i + 1
                );
            }
            s.push_str("</cols>");
        }
        s.push_str("<sheetData>");
        self.zip.write_all(s.as_bytes())
    }

    fn end_sheet(&mut self) -> io::Result<()> {
        if !self.open {
            return Ok(());
        }
        self.open = false;
        let mut s = String::from("</sheetData>");
        if self.header && self.rows > 1 && !self.cols.is_empty() {
            let range = format!("A1:{}{}", col_name(self.cols.len() - 1), self.rows);
            let _ = write!(s, r#"<autoFilter ref="{range}"/>"#);
            if let Some(sh) = self.sheets.last_mut() {
                sh.filter = Some(range);
            }
        }
        s.push_str(r#"<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/></worksheet>"#);
        self.zip.write_all(s.as_bytes())
    }

    /// Opens the next data sheet (and writes the header row into it).
    fn open_data_sheet(&mut self) -> io::Result<()> {
        self.parts += 1;
        let name = if self.parts == 1 {
            self.base.clone()
        } else {
            let suffix = format!(" ({})", self.parts);
            format!(
                "{}{suffix}",
                self.base
                    .chars()
                    .take(31 - suffix.len())
                    .collect::<String>()
            )
        };
        let widths: Vec<(Col, f32)> = self.cols.iter().map(|c| (c.col, width(c.col))).collect();
        self.start_sheet(name, false, &widths, self.header)?;
        self.open = true;
        self.rows = 0;
        if self.header {
            let titles: Vec<Cell> = self
                .cols
                .iter()
                .map(|c| Cell::Text(c.title.clone().into()))
                .collect();
            self.write_row(&titles, Kind::Header, RowLook::default())?;
        }
        Ok(())
    }

    fn write_row(&mut self, cells: &[Cell<'_>], kind: Kind, look: RowLook) -> io::Result<()> {
        self.rows += 1;
        let r = self.rows;
        let (fg, bg) = if self.opts.colors {
            (rgb(look.fg), rgb(look.bg))
        } else {
            (None, None)
        };
        let mut s = std::mem::take(&mut self.buf);
        s.clear();
        let _ = write!(s, r#"<row r="{r}">"#);
        for (i, c) in cells.iter().enumerate() {
            let cell_kind = match (kind, c) {
                (Kind::Header, _) => Kind::Header,
                (_, Cell::Date { .. }) => Kind::Date,
                (_, Cell::Seconds(_)) => Kind::Seconds,
                _ => Kind::General,
            };
            if matches!(c, Cell::Empty) && fg.is_none() && bg.is_none() {
                continue;
            }
            let style = self.styles.get(cell_kind, fg, bg);
            let at = format!("{}{r}", col_name(i));
            let st = if style == 0 {
                String::new()
            } else {
                format!(r#" s="{style}""#)
            };
            match c {
                Cell::Empty => {
                    let _ = write!(s, r#"<c r="{at}"{st}/>"#);
                }
                Cell::Int(v) => {
                    let _ = write!(s, r#"<c r="{at}"{st}><v>{v}</v></c>"#);
                }
                Cell::Seconds(v) => {
                    let _ = write!(s, r#"<c r="{at}"{st}><v>{v}</v></c>"#);
                }
                Cell::Date { ms, .. } => {
                    let _ = write!(s, r#"<c r="{at}"{st}><v>{}</v></c>"#, excel_date(*ms));
                }
                Cell::Text(t) => {
                    let _ = write!(
                        s,
                        r#"<c r="{at}"{st} t="inlineStr"><is><t xml:space="preserve">"#
                    );
                    if t.chars().nth(MAX_CELL_CHARS).is_some() {
                        let cut: String = t.chars().take(MAX_CELL_CHARS - 1).collect();
                        push_xml(&mut s, &cut);
                        s.push('…');
                        self.truncated += 1;
                    } else {
                        push_xml(&mut s, t);
                    }
                    s.push_str("</t></is></c>");
                }
            }
        }
        s.push_str("</row>");
        let res = self.zip.write_all(s.as_bytes());
        self.buf = s;
        res
    }

    /// A small sheet written at once (Summary, Statistics); `front` = before the data sheets.
    #[cfg_attr(not(test), allow(dead_code))] // used by the Summary / Statistics sheets
    pub fn small_sheet(
        &mut self,
        name: &str,
        widths: &[f32],
        rows: &[Vec<Cell<'_>>],
        bold_rows: &[usize],
        front: bool,
    ) -> io::Result<()> {
        self.end_sheet()?;
        let cols: Vec<(Col, f32)> = widths.iter().map(|&w| (Col::Message, w)).collect();
        self.start_sheet(sheet_name(name), front, &cols, false)?;
        let (keep_rows, keep_header) = (self.rows, self.header);
        self.rows = 0;
        self.header = false;
        for (i, r) in rows.iter().enumerate() {
            let kind = if bold_rows.contains(&i) {
                Kind::Header
            } else {
                Kind::General
            };
            self.write_row(r, kind, RowLook::default())?;
        }
        self.rows = keep_rows;
        self.header = keep_header;
        self.zip.write_all(
            br#"</sheetData><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/></worksheet>"#,
        )
    }

    /// Writes the styles and the workbook; returns the output.
    pub fn finish(mut self) -> io::Result<W> {
        if self.sheets.is_empty() {
            // An export without rows and header still needs one sheet.
            self.open_data_sheet()?;
        }
        self.end_sheet()?;
        let mut order: Vec<&Sheet> = self.sheets.iter().filter(|s| s.front).collect();
        order.extend(self.sheets.iter().filter(|s| !s.front));

        let mut ct = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#,
        );
        let mut rels = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        );
        let mut wb = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="{NS_MAIN}" xmlns:r="{NS_REL}"><bookViews><workbookView activeTab="0"/></bookViews><sheets>"#
        );
        let mut names = String::new();
        for (pos, sh) in order.iter().enumerate() {
            let f = sh.file;
            let _ = write!(
                ct,
                r#"<Override PartName="/xl/worksheets/sheet{f}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
            );
            let _ = write!(
                rels,
                r#"<Relationship Id="rId{f}" Type="{NS_REL}/worksheet" Target="worksheets/sheet{f}.xml"/>"#
            );
            wb.push_str(r#"<sheet name=""#);
            push_xml(&mut wb, &sh.name);
            let _ = write!(wb, r#"" sheetId="{f}" r:id="rId{f}"/>"#);
            if let Some(range) = &sh.filter {
                let abs = range
                    .split(':')
                    .map(|p| {
                        let split = p.find(|c: char| c.is_ascii_digit()).unwrap_or(p.len());
                        format!("${}${}", &p[..split], &p[split..])
                    })
                    .collect::<Vec<_>>()
                    .join(":");
                let _ = write!(
                    names,
                    r#"<definedName name="_xlnm._FilterDatabase" localSheetId="{pos}" hidden="1">'"#
                );
                push_xml(&mut names, &sh.name.replace('\'', "''"));
                let _ = write!(names, "'!{abs}</definedName>");
            }
        }
        wb.push_str("</sheets>");
        if !names.is_empty() {
            let _ = write!(wb, "<definedNames>{names}</definedNames>");
        }
        wb.push_str("</workbook>");
        let styles_id = self.sheets.len() + 1;
        let _ = write!(
            rels,
            r#"<Relationship Id="rId{styles_id}" Type="{NS_REL}/styles" Target="styles.xml"/></Relationships>"#
        );
        ct.push_str("</Types>");
        let root_rels = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{NS_REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
        );
        let styles = self.styles.xml();
        self.zip.file("xl/styles.xml", styles.as_bytes())?;
        self.zip.file("xl/workbook.xml", wb.as_bytes())?;
        self.zip
            .file("xl/_rels/workbook.xml.rels", rels.as_bytes())?;
        self.zip.file("_rels/.rels", root_rels.as_bytes())?;
        self.zip.file("[Content_Types].xml", ct.as_bytes())?;
        self.zip.finish()
    }
}

impl<W: Write + Seek> TableSink for XlsxWriter<W> {
    fn header(&mut self, cols: &[ColumnSpec]) -> io::Result<()> {
        self.cols = cols.to_vec();
        self.header = true;
        Ok(())
    }

    fn row(&mut self, cells: &[Cell<'_>], look: RowLook) -> io::Result<()> {
        if self.cols.len() < cells.len() {
            // No header row: the columns are only needed for the widths.
            self.cols = (0..cells.len())
                .map(|_| ColumnSpec {
                    col: Col::Extra(0),
                    title: String::new(),
                })
                .collect();
        }
        if !self.open || self.rows >= self.opts.max_rows {
            self.end_sheet()?;
            self.open_data_sheet()?;
        }
        self.write_row(cells, Kind::General, look)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{sample, specs};
    use super::super::*;
    use super::*;

    fn unzip(data: &[u8]) -> HashMap<String, String> {
        crate::update::unzip(data)
            .unwrap()
            .into_iter()
            .map(|(n, b)| (n, String::from_utf8(b).unwrap()))
            .collect()
    }

    #[test]
    fn names_dates_and_escaping() {
        assert_eq!(
            [0, 25, 26, 27, 701, 702].map(col_name),
            ["A", "Z", "AA", "AB", "ZZ", "AAA"]
        );
        // 2026-10-27 12:00 local = serial 46322.5
        let ms = crate::store::iso_to_ms("2026-10-27 12:00:00.000").unwrap();
        assert!((excel_date(ms) - 46322.5).abs() < 1e-9);
        assert_eq!(sheet_name("a/b:c[1]"), "a_b_c_1_");
        assert_eq!(sheet_name(&"x".repeat(40)).len(), 31);
        let mut s = String::new();
        push_xml(&mut s, "a<b>&\"c\"\u{1}\td");
        assert_eq!(s, "a&lt;b&gt;&amp;&quot;c&quot;\td");
    }

    #[test]
    fn workbook_with_typed_cells_and_colors() {
        let ctx = sample();
        let cols = specs(&[Col::Line, Col::DateTime, Col::Delta, Col::Message]);
        let opts = XlsxOptions {
            colors: true,
            ..Default::default()
        };
        let mut w = XlsxWriter::new(io::Cursor::new(Vec::new()), opts);
        write_table(
            &ctx,
            &Rows::All(3),
            &cols,
            true,
            &mut w,
            &Control::default(),
        )
        .unwrap();
        let files = unzip(&w.finish().unwrap().into_inner());
        for f in [
            "[Content_Types].xml",
            "_rels/.rels",
            "xl/workbook.xml",
            "xl/_rels/workbook.xml.rels",
            "xl/styles.xml",
            "xl/worksheets/sheet1.xml",
        ] {
            assert!(files.contains_key(f), "{f}");
        }
        let sheet = &files["xl/worksheets/sheet1.xml"];
        assert!(sheet.contains(r#"state="frozen""#));
        assert!(sheet.contains(r#"<autoFilter ref="A1:D4"/>"#));
        assert!(sheet.contains(r#"t="inlineStr""#));
        assert!(sheet.contains("<v>1</v>"));
        assert!(sheet.contains("<v>1.5</v>"));
        assert!(sheet.contains("Failed, error 0x80070005\nsecond line"));
        // The formula-like text stays text (no <f> element).
        assert!(sheet.contains(">=SUM(A1)<") && !sheet.contains("<f>"));
        let wb = &files["xl/workbook.xml"];
        assert!(wb.contains(r#"<sheet name="Log" sheetId="1" r:id="rId1"/>"#));
        assert!(wb.contains("'Log'!$A$1:$D$4"));
        let styles = &files["xl/styles.xml"];
        assert!(styles.contains("yyyy\\-mm\\-dd\\ hh:mm:ss.000"));
        assert!(styles.contains(r#"<fgColor rgb="FF010203"/>"#), "{styles}");
        assert!(styles.contains(r#"<color rgb="FF090909"/>"#));
    }

    #[test]
    fn split_over_sheets_and_truncation() {
        let long = "x".repeat(MAX_CELL_CHARS + 10);
        let ctx =
            super::super::tests::context(crate::store::LogStore::from_entries((0..5).map(|i| {
                super::super::tests::entry(i + 1, "", crate::parser::Severity::Info, "", &long)
            })));
        let opts = XlsxOptions {
            max_rows: 3,
            ..Default::default()
        };
        let mut w = XlsxWriter::new(io::Cursor::new(Vec::new()), opts);
        let cols = specs(&[Col::Line, Col::Message]);
        write_table(
            &ctx,
            &Rows::All(5),
            &cols,
            true,
            &mut w,
            &Control::default(),
        )
        .unwrap();
        w.small_sheet(
            "Summary",
            &[20.0, 40.0],
            &[vec![Cell::Text("a".into())]],
            &[0],
            true,
        )
        .unwrap();
        assert_eq!(w.truncated, 5);
        assert_eq!(w.data_sheets(), 3);
        let files = unzip(&w.finish().unwrap().into_inner());
        let wb = &files["xl/workbook.xml"];
        // Summary first, then Log, Log (2), Log (3) – every one with its header.
        let order: Vec<usize> = ["\"Summary\"", "\"Log\"", "\"Log (2)\"", "\"Log (3)\""]
            .iter()
            .map(|n| wb.find(n).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]));
        for f in 1..=3 {
            let s = &files[&format!("xl/worksheets/sheet{f}.xml")];
            assert!(s.contains(">Line<"), "sheet {f}");
        }
        let s = &files["xl/worksheets/sheet1.xml"];
        assert!(s.contains(&format!("{}…<", "x".repeat(20))));
        assert!(!s.contains(&"x".repeat(MAX_CELL_CHARS)));
        assert!(wb.contains(r#"localSheetId="1""#));
    }

    /// Writes a sample workbook (every column, colors, a split, a cut cell) for a check in Excel /
    /// LibreOffice: `RUSTRACE_XLSX_OUT=<file> cargo test xlsx_sample -- --ignored`.
    #[test]
    #[ignore]
    fn xlsx_sample() {
        let Ok(out) = std::env::var("RUSTRACE_XLSX_OUT") else {
            return;
        };
        let mut entries: Vec<_> = (0..6)
            .map(|i| {
                super::super::tests::entry(
                    i + 1,
                    &format!("2026-10-27 19:49:0{i}.123"),
                    crate::parser::Severity::Info,
                    "Comp <&>",
                    "Download \"quoted\" é ž",
                )
            })
            .collect();
        entries[2].severity = crate::parser::Severity::Error;
        entries[2].message = "Failed 0x80070005\nsecond line".into();
        entries[3].message = "=SUM(A1)".into();
        entries[4].message = "y".repeat(MAX_CELL_CHARS + 5);
        let ctx = super::super::tests::context(crate::store::LogStore::from_entries(entries));
        let all = [
            Col::Line,
            Col::File,
            Col::Path,
            Col::DateTime,
            Col::Delta,
            Col::Severity,
            Col::Component,
            Col::Thread,
            Col::Message,
            Col::Rule,
            Col::Note,
            Col::Codes,
            Col::Block,
        ];
        let opts = XlsxOptions {
            colors: true,
            max_rows: 5,
            ..Default::default()
        };
        let file = std::fs::File::create(out).unwrap();
        let mut w = XlsxWriter::new(file, opts);
        write_table(
            &ctx,
            &Rows::Blocks(vec![(0, 5)]),
            &specs(&all),
            true,
            &mut w,
            &Control::default(),
        )
        .unwrap();
        w.finish().unwrap();
    }
}
