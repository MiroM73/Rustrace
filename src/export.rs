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

// ----------------------------------------------------------------------
// Time filter and block limit
// ----------------------------------------------------------------------

/// Timestamp of an entry as a comparable ISO string ("YYYY-MM-DD HH:MM:SS.mmm"),
/// or `None` when the log line has no date/time.
pub fn timestamp(e: &LogEntry) -> Option<&str> {
    let b = e.datetime.as_bytes();
    (b.len() >= 10 && b[4] == b'-' && b[7] == b'-' && b[..4].iter().all(u8::is_ascii_digit))
        .then_some(e.datetime.as_str())
}

/// Newest timestamp in the log (entries are not always strictly ordered).
pub fn latest_timestamp(entries: &[LogEntry]) -> Option<String> {
    entries.iter().filter_map(timestamp).max().map(str::to_owned)
}

/// Parses a user-entered date/time into the ISO form used by the parser.
///
/// Accepts ISO (`2026-10-23 13:05`, `2026-10-23T13:05:30`) and the date order of the
/// UI language given by `date_pattern` (e.g. `{d}.{m}.{y}` → `23.10.2026 13:05`).
/// The time is optional (default 00:00:00); seconds and milliseconds are optional.
pub fn parse_datetime_input(input: &str, date_pattern: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let (date_part, time_part) = match input.split_once(['T', ' ']) {
        Some((d, t)) => (d.trim(), t.trim()),
        None => (input, ""),
    };
    let nums: Vec<&str> = date_part
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .collect();
    if nums.len() != 3 {
        return None;
    }
    // Order of the parts: ISO when the first part has 4 digits, otherwise the language pattern.
    let order: Vec<char> = if nums[0].len() == 4 {
        vec!['y', 'm', 'd']
    } else {
        let mut o: Vec<(usize, char)> = ['y', 'm', 'd']
            .into_iter()
            .filter_map(|k| date_pattern.find(&format!("{{{k}}}")).map(|pos| (pos, k)))
            .collect();
        o.sort();
        o.into_iter().map(|(_, k)| k).collect()
    };
    if order.len() != 3 {
        return None;
    }
    let mut y = 0i32;
    let mut m = 0u32;
    let mut d = 0u32;
    for (part, kind) in nums.iter().zip(&order) {
        let v: u32 = part.parse().ok()?;
        match kind {
            'y' => y = if part.len() == 2 { 2000 + v as i32 } else { v as i32 },
            'm' => m = v,
            _ => d = v,
        }
    }
    let date = chrono::NaiveDate::from_ymd_opt(y, m, d)?;

    let (mut hh, mut mm, mut ss, mut ms) = (0u32, 0u32, 0u32, 0u32);
    if !time_part.is_empty() {
        let (hms, frac) = match time_part.split_once(['.', ',']) {
            Some((a, b)) => (a, Some(b)),
            None => (time_part, None),
        };
        let parts: Vec<&str> = hms.split(':').collect();
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        let vals: Vec<u32> = parts
            .iter()
            .map(|p| p.trim().parse().ok())
            .collect::<Option<Vec<_>>>()?;
        hh = vals[0];
        mm = vals.get(1).copied().unwrap_or(0);
        ss = vals.get(2).copied().unwrap_or(0);
        if let Some(f) = frac {
            let f: String = f.chars().take(3).collect();
            ms = format!("{f:0<3}").parse().ok()?;
        }
    }
    chrono::NaiveTime::from_hms_milli_opt(hh, mm, ss, ms)?;
    Some(format!(
        "{} {hh:02}:{mm:02}:{ss:02}.{ms:03}",
        date.format("%Y-%m-%d")
    ))
}

/// ISO timestamp `hours` before the given ISO timestamp (for "last hour" style presets).
pub fn hours_before(iso: &str, hours: i64) -> Option<String> {
    let t = chrono::NaiveDateTime::parse_from_str(&iso[..iso.len().min(23)], "%Y-%m-%d %H:%M:%S%.3f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(&iso[..iso.len().min(19)], "%Y-%m-%d %H:%M:%S"))
        .ok()?;
    Some((t - chrono::Duration::hours(hours)).format("%Y-%m-%d %H:%M:%S%.3f").to_string())
}

/// Keeps only hits at or after `since`. Entries without a timestamp are dropped
/// when a time filter is active (their time is unknown).
pub fn filter_hits_since(hits: &[usize], entries: &[LogEntry], since: Option<&str>) -> Vec<usize> {
    match since {
        None => hits.to_vec(),
        Some(since) => hits
            .iter()
            .copied()
            .filter(|&i| timestamp(&entries[i]).is_some_and(|t| t >= since))
            .collect(),
    }
}

/// Removes context rows older than `since` from the start of every block, so the export
/// does not contain records from before the chosen time. Rows without a timestamp
/// (e.g. plain lines inside a CMTrace log) are kept.
pub fn clip_blocks_since(
    blocks: &[(usize, usize)],
    entries: &[LogEntry],
    since: Option<&str>,
) -> Vec<(usize, usize)> {
    let Some(since) = since else { return blocks.to_vec() };
    blocks
        .iter()
        .filter_map(|&(a, b)| {
            let start = (a..=b).find(|&i| timestamp(&entries[i]).is_none_or(|t| t >= since))?;
            Some((start, b))
        })
        .collect()
}

/// Keeps only the last `n` blocks (the newest ones).
pub fn keep_last_blocks(mut blocks: Vec<(usize, usize)>, n: Option<usize>) -> Vec<(usize, usize)> {
    if let Some(n) = n
        && blocks.len() > n
    {
        blocks.drain(..blocks.len() - n);
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

    fn timed(i: usize, time: &str) -> LogEntry {
        LogEntry {
            line_no: i + 1,
            message: format!("m{i}"),
            component: String::new(),
            datetime: time.to_owned(),
            thread: String::new(),
            severity: Severity::Info,
        }
    }

    #[test]
    fn datetime_input_parsing() {
        let sk = "{d}.{m}.{y}";
        let en = "{y}-{m}-{d}";
        assert_eq!(parse_datetime_input("23.10.2026 13:05", sk).unwrap(), "2026-10-23 13:05:00.000");
        assert_eq!(parse_datetime_input("2026-10-23", sk).unwrap(), "2026-10-23 00:00:00.000");
        assert_eq!(parse_datetime_input("2026-10-23T13:05:30.5", en).unwrap(), "2026-10-23 13:05:30.500");
        assert_eq!(parse_datetime_input(" 3.1.26 7:08:09 ", sk).unwrap(), "2026-01-03 07:08:09.000");
        assert_eq!(parse_datetime_input("10/23/2026 1:02", "{m}/{d}/{y}").unwrap(), "2026-10-23 01:02:00.000");
        assert!(parse_datetime_input("31.02.2026", sk).is_none());
        assert!(parse_datetime_input("23.10.2026 25:00", sk).is_none());
        assert!(parse_datetime_input("yesterday", sk).is_none());
        assert_eq!(hours_before("2026-10-23 00:30:00.000", 1).unwrap(), "2026-10-22 23:30:00.000");
    }

    #[test]
    fn time_filter_and_last_blocks() {
        let e: Vec<LogEntry> = (0..10)
            .map(|i| timed(i, &format!("2026-10-23 10:00:{:02}.000", i)))
            .chain([timed(10, "")]) // a line without a timestamp
            .collect();
        let since = Some("2026-10-23 10:00:05.000");
        assert_eq!(filter_hits_since(&[2, 5, 8, 10], &e, since), vec![5, 8]);
        assert_eq!(filter_hits_since(&[2, 5], &e, None), vec![2, 5]);
        // block 3..=7 is clipped to start at entry 5; block 0..=2 disappears completely
        assert_eq!(clip_blocks_since(&[(0, 2), (3, 7), (8, 10)], &e, since), vec![(5, 7), (8, 10)]);
        assert_eq!(latest_timestamp(&e).unwrap(), "2026-10-23 10:00:09.000");

        let b = vec![(0, 1), (3, 4), (6, 7)];
        assert_eq!(keep_last_blocks(b.clone(), Some(2)), vec![(3, 4), (6, 7)]);
        assert_eq!(keep_last_blocks(b.clone(), Some(5)), b);
        assert_eq!(keep_last_blocks(b.clone(), None), b);
        assert!(keep_last_blocks(b, Some(0)).is_empty());
    }
}
