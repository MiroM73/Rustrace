//! JSON Lines writer of the table export (#32, part 4): one JSON object per row – for scripts,
//! Log Analytics or Splunk ingestion. Keys are stable machine names (`line`, `datetime`,
//! `message`, the extra column names …); dates are always ISO; empty cells are left out.

use super::{Cell, ColumnSpec, RowLook, TableSink};
use crate::store::ms_to_iso;
use std::io::{self, Write};

pub struct JsonlWriter<W: Write> {
    out: W,
    keys: Vec<String>,
    line: String,
}

impl<W: Write> JsonlWriter<W> {
    /// `keys` = the JSON key of every column (`Col::json_key`).
    pub fn new(out: W, keys: Vec<String>) -> Self {
        Self {
            out,
            keys,
            line: String::new(),
        }
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

impl<W: Write> TableSink for JsonlWriter<W> {
    fn header(&mut self, _cols: &[ColumnSpec]) -> io::Result<()> {
        Ok(())
    }

    fn row(&mut self, cells: &[Cell<'_>], _look: RowLook) -> io::Result<()> {
        self.line.clear();
        self.line.push('{');
        let mut first = true;
        for (key, c) in self.keys.iter().zip(cells) {
            let value = match c {
                Cell::Empty => continue,
                Cell::Text(t) => json_str(t),
                Cell::Int(i) => i.to_string(),
                Cell::Seconds(s) => format!("{s:.3}"),
                Cell::Date { ms, .. } => json_str(&ms_to_iso(*ms)),
            };
            if !first {
                self.line.push(',');
            }
            first = false;
            self.line.push_str(&json_str(key));
            self.line.push(':');
            self.line.push_str(&value);
        }
        self.line.push_str("}\n");
        self.out.write_all(self.line.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{sample, specs};
    use super::super::*;
    use super::*;

    #[test]
    fn one_object_per_row() {
        let ctx = sample();
        let cols = [
            Col::Line,
            Col::DateTime,
            Col::Delta,
            Col::Message,
            Col::Note,
        ];
        let keys = cols.iter().map(|c| c.json_key(&[])).collect();
        let mut w = JsonlWriter::new(Vec::new(), keys);
        write_table(
            &ctx,
            &Rows::All(3),
            &specs(&cols),
            true,
            &mut w,
            &Control::default(),
        )
        .unwrap();
        let text = String::from_utf8(w.finish().unwrap()).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["line"], 1);
        assert_eq!(lines[0]["datetime"], "2026-10-27 19:49:05.123");
        assert!(lines[0].get("delta").is_none());
        assert_eq!(lines[1]["delta"], 1.5);
        assert_eq!(lines[1]["message"], "Failed, error 0x80070005\nsecond line");
        assert_eq!(lines[1]["note"], "check this");
        assert_eq!(lines[2]["message"], "=SUM(A1)");
    }
}
