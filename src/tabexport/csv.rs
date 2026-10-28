//! CSV writer of the table export (#32): RFC 4180 quoting, separator, encoding, protection
//! against formula injection.

use super::{Cell, ColumnSpec, RowLook, TableSink};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

/// Text encoding of the CSV file.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum CsvEncoding {
    /// UTF-8 with a byte order mark – Excel recognizes it on a double click.
    #[default]
    Utf8Bom,
    Utf8,
    Utf16Le,
}

#[derive(Clone, Copy, Debug)]
pub struct CsvOptions {
    pub separator: char,
    pub encoding: CsvEncoding,
    /// Values starting with `=`, `+`, `-`, `@`, tab or CR get a leading `'`.
    pub safe_for_excel: bool,
    /// Decimal separator of numbers with a fraction (`,` with the `;` separator).
    pub decimal: char,
}

impl CsvOptions {
    pub fn new(separator: char, encoding: CsvEncoding, safe_for_excel: bool) -> Self {
        Self {
            separator,
            encoding,
            safe_for_excel,
            decimal: if separator == ';' { ',' } else { '.' },
        }
    }
}

pub struct CsvWriter<W: Write> {
    out: W,
    opts: CsvOptions,
    line: String,
    /// A value was written on the current line (the next one needs a separator).
    started: bool,
}

impl<W: Write> CsvWriter<W> {
    /// Starts the file (writes the byte order mark of the encoding).
    pub fn new(mut out: W, opts: CsvOptions) -> io::Result<Self> {
        match opts.encoding {
            CsvEncoding::Utf8Bom => out.write_all(b"\xEF\xBB\xBF")?,
            CsvEncoding::Utf16Le => out.write_all(b"\xFF\xFE")?,
            CsvEncoding::Utf8 => {}
        }
        Ok(Self {
            out,
            opts,
            line: String::new(),
            started: false,
        })
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }

    fn field(&mut self, value: &str, guard: bool) {
        if self.started {
            self.line.push(self.opts.separator);
        }
        self.started = true;
        push_field(
            &mut self.line,
            value,
            self.opts.separator,
            guard && self.opts.safe_for_excel,
        );
    }

    fn end_line(&mut self) -> io::Result<()> {
        self.line.push_str("\r\n");
        match self.opts.encoding {
            CsvEncoding::Utf16Le => {
                let mut b = Vec::with_capacity(self.line.len() * 2);
                for u in self.line.encode_utf16() {
                    b.extend_from_slice(&u.to_le_bytes());
                }
                self.out.write_all(&b)?;
            }
            _ => self.out.write_all(self.line.as_bytes())?,
        }
        self.line.clear();
        self.started = false;
        Ok(())
    }
}

/// Appends one value: a leading `'` for values Excel would take as a formula, quotes around
/// values with the separator, a quote or a line break (quotes doubled); `\r\n` → `\n`.
pub fn push_field(out: &mut String, value: &str, sep: char, guard: bool) {
    let value = if value.contains('\r') {
        std::borrow::Cow::Owned(value.replace("\r\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(value)
    };
    let formula = guard && value.starts_with(['=', '+', '-', '@', '\t', '\r']);
    let quote = value.contains([sep, '"', '\n', '\r']);
    if quote {
        out.push('"');
    }
    if formula {
        out.push('\'');
    }
    for c in value.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    if quote {
        out.push('"');
    }
}

impl<W: Write> TableSink for CsvWriter<W> {
    fn header(&mut self, cols: &[ColumnSpec]) -> io::Result<()> {
        for c in cols {
            self.field(&c.title, false);
        }
        self.end_line()
    }

    fn row(&mut self, cells: &[Cell<'_>], _look: RowLook) -> io::Result<()> {
        let decimal = self.opts.decimal;
        for c in cells {
            // Numbers and dates are never formulas; only free text is guarded.
            let guard = matches!(c, Cell::Text(_));
            let t = c.text(decimal).into_owned();
            self.field(&t, guard);
        }
        self.end_line()
    }
}

/// The list separator of the user's Windows locale (`;` for SK / CS / DE, `,` for EN-US);
/// `,` elsewhere or when it cannot be read.
pub fn system_list_separator() -> char {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetLocaleInfoEx(name: *const u16, lctype: u32, data: *mut u16, len: i32) -> i32;
        }
        const LOCALE_SLIST: u32 = 0x0C;
        let mut buf = [0u16; 8];
        // SAFETY: a null name = the user's default locale; the buffer length is passed.
        let n = unsafe {
            GetLocaleInfoEx(
                std::ptr::null(),
                LOCALE_SLIST,
                buf.as_mut_ptr(),
                buf.len() as i32,
            )
        };
        if n > 1
            && let Some(Ok(c)) = char::decode_utf16(buf[..n as usize - 1].iter().copied()).next()
            && matches!(c, ',' | ';' | '\t' | '|')
        {
            return c;
        }
    }
    ','
}

#[cfg(test)]
mod tests {
    use super::super::tests::{sample, specs};
    use super::super::*;
    use super::*;

    fn csv(sep: char, enc: CsvEncoding, safe: bool, cols: &[Col]) -> Vec<u8> {
        let ctx = sample();
        let mut w = CsvWriter::new(Vec::new(), CsvOptions::new(sep, enc, safe)).unwrap();
        write_table(
            &ctx,
            &Rows::All(3),
            &specs(cols),
            true,
            &mut w,
            &Control::default(),
        )
        .unwrap();
        w.finish().unwrap()
    }

    #[test]
    fn quoting_and_formula_guard() {
        let mut s = String::new();
        push_field(&mut s, "a;b", ';', false);
        assert_eq!(s, "\"a;b\"");
        s.clear();
        push_field(&mut s, "say \"hi\"", ',', false);
        assert_eq!(s, "\"say \"\"hi\"\"\"");
        s.clear();
        push_field(&mut s, "one\r\ntwo", ',', false);
        assert_eq!(s, "\"one\ntwo\"");
        s.clear();
        push_field(&mut s, "a,b", ';', false);
        assert_eq!(s, "a,b");
        for (v, out) in [
            ("=1+1", "'=1+1"),
            ("+x", "'+x"),
            ("-x", "'-x"),
            ("@SUM(A1)", "'@SUM(A1)"),
            ("\tx", "'\tx"),
            ("x=1", "x=1"),
        ] {
            s.clear();
            push_field(&mut s, v, ',', true);
            assert_eq!(s, out, "{v}");
        }
        s.clear();
        push_field(&mut s, "=a;b", ';', true);
        assert_eq!(s, "\"'=a;b\"");
    }

    #[test]
    fn whole_file_separators_and_encodings() {
        let cols = [Col::Line, Col::DateTime, Col::Delta, Col::Message];
        let b = csv(';', CsvEncoding::Utf8Bom, true, &cols);
        assert!(b.starts_with(b"\xEF\xBB\xBF"));
        let t = String::from_utf8(b[3..].to_vec()).unwrap();
        assert_eq!(
            t,
            "Line;DateTime;Delta;Message\r\n\
             1;2026-10-27 19:49:05.123;;Download started\r\n\
             2;2026-10-27 19:49:06.623;1,500;\"Failed, error 0x80070005\nsecond line\"\r\n\
             3;;;'=SUM(A1)\r\n"
        );
        let t = String::from_utf8(csv(',', CsvEncoding::Utf8, false, &cols)).unwrap();
        assert!(t.starts_with("Line,DateTime"));
        assert!(t.contains(",1.500,\"Failed, error"));
        assert!(t.ends_with("3,,,=SUM(A1)\r\n"));
        let b = csv('\t', CsvEncoding::Utf16Le, true, &[Col::Line, Col::Message]);
        assert_eq!(&b[..2], b"\xFF\xFE");
        let units: Vec<u16> = b[2..]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let t = String::from_utf16(&units).unwrap();
        assert!(t.starts_with("Line\tMessage\r\n1\tDownload started\r\n"));
    }

    #[test]
    fn list_separator_is_a_separator() {
        assert!(matches!(system_list_separator(), ',' | ';' | '\t' | '|'));
    }
}
