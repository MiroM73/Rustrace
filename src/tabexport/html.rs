//! HTML report of the table export (#32, part 4): one self-contained file (no scripts, no
//! external files) with a colored table – for e-mails or tickets where Excel is not wanted.
//! Multi-line cells are collapsible (`<details>`).

use super::{Cell, ColumnSpec, RowLook, TableSink};
use std::io::{self, Write};

pub struct HtmlWriter<W: Write> {
    out: W,
    colors: bool,
    started: bool,
    buf: String,
}

fn esc(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn css(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

impl<W: Write> HtmlWriter<W> {
    /// Writes the start of the page: `title` as the heading, `info` as lines under it.
    pub fn new(mut out: W, title: &str, info: &[String], colors: bool) -> io::Result<Self> {
        let mut s = String::from("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>");
        esc(&mut s, title);
        s.push_str(
            "</title><style>\
             body{font-family:Segoe UI,Arial,sans-serif;font-size:13px;margin:16px;color:#222}\
             h1{font-size:18px;margin:0 0 6px}p.info{margin:2px 0;color:#555}\
             table{border-collapse:collapse;margin-top:12px}\
             th,td{border:1px solid #ccc;padding:2px 6px;vertical-align:top;text-align:left}\
             th{background:#eee;position:sticky;top:0}\
             td.n{text-align:right;white-space:nowrap}td.d{white-space:nowrap}\
             pre{margin:0;font-family:inherit;white-space:pre-wrap}\
             details summary{cursor:pointer}\
             </style></head><body>\n<h1>",
        );
        esc(&mut s, title);
        s.push_str("</h1>\n");
        for line in info {
            s.push_str("<p class=\"info\">");
            esc(&mut s, line);
            s.push_str("</p>\n");
        }
        s.push_str("<table>\n");
        out.write_all(s.as_bytes())?;
        Ok(Self {
            out,
            colors,
            started: false,
            buf: String::new(),
        })
    }

    pub fn finish(mut self) -> io::Result<W> {
        if self.started {
            self.out.write_all(b"</tbody>\n")?;
        }
        self.out.write_all(b"</table>\n</body></html>\n")?;
        self.out.flush()?;
        Ok(self.out)
    }

    fn body(&mut self) -> io::Result<()> {
        if !self.started {
            self.started = true;
            self.out.write_all(b"<tbody>\n")?;
        }
        Ok(())
    }
}

impl<W: Write> TableSink for HtmlWriter<W> {
    fn header(&mut self, cols: &[ColumnSpec]) -> io::Result<()> {
        let mut s = String::from("<thead><tr>");
        for c in cols {
            s.push_str("<th>");
            esc(&mut s, &c.title);
            s.push_str("</th>");
        }
        s.push_str("</tr></thead>\n");
        self.out.write_all(s.as_bytes())
    }

    fn row(&mut self, cells: &[Cell<'_>], look: RowLook) -> io::Result<()> {
        self.body()?;
        let mut s = std::mem::take(&mut self.buf);
        s.clear();
        s.push_str("<tr");
        if self.colors && (look.fg.is_some() || look.bg.is_some()) {
            s.push_str(" style=\"");
            if let Some(c) = look.fg {
                s.push_str(&format!("color:{};", css(c)));
            }
            if let Some(c) = look.bg {
                s.push_str(&format!("background:{};", css(c)));
            }
            s.push('"');
        }
        s.push('>');
        for c in cells {
            let class = match c {
                Cell::Int(_) | Cell::Seconds(_) => " class=\"n\"",
                Cell::Date { .. } => " class=\"d\"",
                _ => "",
            };
            s.push_str("<td");
            s.push_str(class);
            s.push('>');
            let text = c.text('.');
            match text.split_once('\n') {
                // Multi-line: the first line opens the rest.
                Some((first, rest)) => {
                    s.push_str("<details><summary>");
                    esc(&mut s, first.trim_end_matches('\r'));
                    s.push_str("</summary><pre>");
                    esc(&mut s, rest);
                    s.push_str("</pre></details>");
                }
                None => esc(&mut s, &text),
            }
            s.push_str("</td>");
        }
        s.push_str("</tr>\n");
        let res = self.out.write_all(s.as_bytes());
        self.buf = s;
        res
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{sample, specs};
    use super::super::*;
    use super::*;

    #[test]
    fn report_with_colors_and_details() {
        let ctx = sample();
        let mut w =
            HtmlWriter::new(Vec::new(), "a.log <export>", &["3 rows".into()], true).unwrap();
        write_table(
            &ctx,
            &Rows::All(3),
            &specs(&[Col::Line, Col::Message]),
            true,
            &mut w,
            &Control::default(),
        )
        .unwrap();
        let html = String::from_utf8(w.finish().unwrap()).unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<title>a.log &lt;export&gt;</title>"));
        assert!(html.contains("<p class=\"info\">3 rows</p>"));
        assert!(html.contains("<th>Message</th>"));
        assert!(html.contains("<tr style=\"background:#010203;\">"));
        assert!(html.contains("<tr style=\"color:#090909;background:#080808;\">"));
        assert!(html.contains(
            "<details><summary>Failed, error 0x80070005</summary><pre>second line</pre></details>"
        ));
        assert!(html.contains("<td class=\"n\">1</td>"));
        assert!(html.trim_end().ends_with("</body></html>"));
        assert!(!html.contains("<script"));
    }
}
