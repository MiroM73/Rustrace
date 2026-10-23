//! Export of highlighted rows together with surrounding context lines.
//!
//! For every highlighted entry the range `[index - before, index + after]` is taken.
//! Overlapping or touching ranges are merged, so every entry is exported at most once.
//! The result is a list of blocks; blocks can be separated in the output.

use crate::parser::LogEntry;
use serde::{Deserialize, Serialize};

/// Output format of the export.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum ExportFormat {
    /// The original lines from the log file – opens again in CMTrace / RusTrace.
    Original,
    /// Readable tab-separated table: marker, line, date/time, severity, component, thread, message.
    Readable,
}

/// Merges `[hit - before, hit + after]` ranges of entry indexes into non-overlapping
/// inclusive blocks. `hits` must be sorted ascending; `len` is the number of entries.
pub fn merge_ranges(hits: &[usize], before: usize, after: usize, len: usize) -> Vec<(usize, usize)> {
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    if len == 0 {
        return blocks;
    }
    for &h in hits {
        if h >= len {
            continue;
        }
        let start = h.saturating_sub(before);
        let end = h.saturating_add(after).min(len - 1);
        match blocks.last_mut() {
            // Overlapping or directly adjacent → extend the previous block (no duplicates).
            Some(last) if start <= last.1.saturating_add(1) => last.1 = last.1.max(end),
            _ => blocks.push((start, end)),
        }
    }
    blocks
}

/// Number of entries covered by the blocks.
pub fn row_count(blocks: &[(usize, usize)]) -> usize {
    blocks.iter().map(|(a, b)| b - a + 1).sum()
}

/// Physical file lines (1-based, inclusive) covered by a block of entries.
pub fn line_range(entries: &[LogEntry], block: (usize, usize)) -> (usize, usize) {
    let (a, b) = block;
    let first = entries[a].line_no;
    let last = match entries.get(b + 1) {
        Some(next) => next.line_no.saturating_sub(1).max(entries[b].line_no),
        None => entries[b].line_no + entries[b].message.lines().count().max(1) - 1,
    };
    (first, last)
}

/// Separator line written between blocks.
pub fn separator(first_line: usize, last_line: usize) -> String {
    format!("-------- {first_line} – {last_line} --------")
}

/// Builds the output of the "original lines" format from lines already read from the file.
pub fn build_original(
    line_ranges: &[(usize, usize)],
    lines: &[Vec<String>],
    with_separators: bool,
) -> String {
    let mut out = String::new();
    for (i, ((first, last), block)) in line_ranges.iter().zip(lines).enumerate() {
        if with_separators {
            if i > 0 {
                out.push_str("\r\n");
            }
            out.push_str(&separator(*first, *last));
            out.push_str("\r\n");
        }
        for l in block {
            out.push_str(l);
            out.push_str("\r\n");
        }
    }
    out
}

/// One row of the readable format (all texts already localized / formatted).
pub struct ReadableRow<'a> {
    pub highlighted: bool,
    pub line: usize,
    pub datetime: String,
    pub severity: String,
    pub entry: &'a LogEntry,
}

/// Builds the readable tab-separated table. `header` is the localized column header line.
pub fn build_readable(
    header: &str,
    blocks: &[Vec<ReadableRow<'_>>],
    line_ranges: &[(usize, usize)],
    with_separators: bool,
) -> String {
    let mut out = String::new();
    out.push_str(header);
    out.push_str("\r\n");
    for (i, (rows, (first, last))) in blocks.iter().zip(line_ranges).enumerate() {
        if with_separators {
            if i > 0 {
                out.push_str("\r\n");
            }
            out.push_str(&separator(*first, *last));
            out.push_str("\r\n");
        }
        for r in rows {
            let msg = r.entry.message.replace("\r\n", "\n").replace('\n', " ↵ ");
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\r\n",
                if r.highlighted { "»" } else { "" },
                r.line,
                r.datetime,
                r.severity,
                r.entry.component,
                r.entry.thread,
                msg
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Severity;

    fn entry(line_no: usize, msg: &str) -> LogEntry {
        LogEntry {
            line_no,
            message: msg.to_owned(),
            component: String::new(),
            datetime: String::new(),
            thread: String::new(),
            severity: Severity::Info,
        }
    }

    #[test]
    fn merge_overlapping_and_adjacent() {
        // hits 5 and 8 with ±2 overlap → one block; 20 is separate; clamped at both ends.
        assert_eq!(merge_ranges(&[5, 8, 20], 2, 2, 100), vec![(3, 10), (18, 22)]);
        // adjacent blocks (3..=5 and 6..=8) are merged too
        assert_eq!(merge_ranges(&[4, 7], 1, 1, 100), vec![(3, 8)]);
        // clamping
        assert_eq!(merge_ranges(&[0, 99], 5, 5, 100), vec![(0, 5), (94, 99)]);
        // zero context exports only the hits
        assert_eq!(merge_ranges(&[1, 2, 5], 0, 0, 10), vec![(1, 2), (5, 5)]);
        assert!(merge_ranges(&[1], 3, 3, 0).is_empty());
        let b = merge_ranges(&[5, 8, 20], 2, 2, 100);
        assert_eq!(row_count(&b), 8 + 5);
    }

    #[test]
    fn line_ranges_with_multiline_entries() {
        // entry 1 spans lines 2-4 (multi-line message)
        let e = vec![entry(1, "a"), entry(2, "b\nc\nd"), entry(5, "e"), entry(6, "f\ng")];
        assert_eq!(line_range(&e, (0, 1)), (1, 4));
        assert_eq!(line_range(&e, (2, 3)), (5, 7));
    }

    #[test]
    fn outputs() {
        let ranges = vec![(1, 2), (10, 10)];
        let lines = vec![vec!["a".to_owned(), "b".to_owned()], vec!["x".to_owned()]];
        assert_eq!(build_original(&ranges, &lines, false), "a\r\nb\r\nx\r\n");
        let with_sep = build_original(&ranges, &lines, true);
        assert!(with_sep.starts_with("-------- 1 – 2 --------\r\na\r\nb\r\n\r\n-------- 10 – 10"));

        let e = entry(7, "two\nlines");
        let rows = vec![vec![ReadableRow {
            highlighted: true,
            line: 7,
            datetime: "t".into(),
            severity: "Error".into(),
            entry: &e,
        }]];
        let txt = build_readable("H", &rows, &[(7, 8)], false);
        assert_eq!(txt, "H\r\n»\t7\tt\tError\t\t\ttwo ↵ lines\r\n");
    }

    /// Whole pipeline on a real file: parse, find errors, merge ±1 context, read original lines.
    #[test]
    fn end_to_end_original_export() {
        use crate::parser::{LogSource, PollResult, Progress};
        let dir = std::env::temp_dir().join(format!("rustrace_export_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.log");
        let cm = |m: &str, t: u8| {
            format!("<![LOG[{m}]LOG]!><time=\"10:00:00.000+000\" date=\"10-23-2026\" component=\"C\" context=\"\" type=\"{t}\" thread=\"1\" file=\"\">")
        };
        let lines = [
            cm("one", 1),                // line 1, entry 0
            cm("two", 1),                // line 2, entry 1
            cm("ERR A", 3),              // line 3, entry 2  (hit)
            cm("four", 1),               // line 4, entry 3
            cm("ERR B", 3),              // line 5, entry 4  (hit, overlaps with A's context)
            "<![LOG[multi".to_owned(),   // lines 6-7, entry 5 (context of B)
            "line]LOG]!><time=\"10:00:00.000+000\" date=\"10-23-2026\" component=\"C\" context=\"\" type=\"1\" thread=\"1\" file=\"\">".to_owned(),
            cm("eight", 1),              // line 8, entry 6
            cm("nine", 1),               // line 9, entry 7
            cm("ERR C", 3),              // line 10, entry 8 (hit, separate block)
        ];
        std::fs::write(&p, lines.join("\r\n") + "\r\n").unwrap();

        let mut src = LogSource::new(&p, false);
        let PollResult::Reset { committed: entries, .. } = src.poll(&Progress::default()).unwrap()
        else {
            panic!()
        };
        assert_eq!(entries.len(), 9);
        let hits: Vec<usize> = (0..entries.len())
            .filter(|&i| entries[i].severity == Severity::Error)
            .collect();
        assert_eq!(hits, vec![2, 4, 8]);

        let blocks = merge_ranges(&hits, 1, 1, entries.len());
        assert_eq!(blocks, vec![(1, 5), (7, 8)]); // entry 3 is shared by A and B only once
        let ranges: Vec<_> = blocks.iter().map(|&b| line_range(&entries, b)).collect();
        assert_eq!(ranges, vec![(2, 7), (9, 10)]);

        let raw = crate::parser::read_line_ranges(&p, &ranges).unwrap();
        let out = build_original(&ranges, &raw, false);
        let expected: Vec<String> = [&lines[1..7], &lines[8..10]].concat();
        assert_eq!(out, expected.join("\r\n") + "\r\n");

        // The exported file parses again into the same entries (no duplicates).
        let q = dir.join("out.log");
        std::fs::write(&q, &out).unwrap();
        let mut src2 = LogSource::new(&q, false);
        let PollResult::Reset { committed: again, .. } = src2.poll(&Progress::default()).unwrap()
        else {
            panic!()
        };
        let msgs: Vec<&str> = again.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(msgs, ["two", "ERR A", "four", "ERR B", "multi\nline", "nine", "ERR C"]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
