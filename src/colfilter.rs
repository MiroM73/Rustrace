//! Filters of single table columns (the filter row under the column headers).
//!
//! Syntax of a filter text (always case-insensitive):
//! * `text` – the value contains `text`,
//! * `=text` – the value is exactly `text`,
//! * `a|b|c` – any of the alternatives (each one may start with `=`),
//! * `!…` at the start – negation: the value matches none of the alternatives.
//!
//! Spaces around alternatives are ignored; an empty filter (or only `!`) filters nothing.

use crate::store::{EntryRef, iso_to_ms, ms_to_iso, ms_to_iso_bytes};
use regex::{Regex, RegexBuilder};

#[derive(Clone)]
pub struct ColumnFilter {
    re: Regex,
    negate: bool,
}

/// One alternative of a filter text: the text and whether it must match exactly (`=`).
struct Alternative<'a> {
    text: &'a str,
    exact: bool,
}

/// Splits a filter text into the negation flag and its alternatives (none = filters nothing).
fn split(input: &str) -> (bool, Vec<Alternative<'_>>) {
    let input = input.trim();
    let (negate, body) = match input.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, input),
    };
    let alternatives = body
        .split('|')
        .map(str::trim)
        .filter(|a| !a.is_empty() && *a != "=")
        .map(|a| match a.strip_prefix('=') {
            Some(exact) => Alternative {
                text: exact.trim(),
                exact: true,
            },
            None => Alternative {
                text: a,
                exact: false,
            },
        })
        .collect();
    (negate, alternatives)
}

impl ColumnFilter {
    /// Compiles a filter text; `None` when it filters nothing.
    pub fn parse(input: &str) -> Option<ColumnFilter> {
        let (negate, alternatives) = split(input);
        let alternatives: Vec<String> = alternatives
            .iter()
            .map(|a| match a.exact {
                true => format!("^{}$", regex::escape(a.text)),
                false => regex::escape(a.text),
            })
            .collect();
        if alternatives.is_empty() {
            return None;
        }
        let re = RegexBuilder::new(&format!("(?:{})", alternatives.join("|")))
            .case_insensitive(true)
            .build()
            .ok()?;
        Some(ColumnFilter { re, negate })
    }

    pub fn matches(&self, value: &str) -> bool {
        self.re.is_match(value) != self.negate
    }
}

// ----------------------------------------------------------------------
// Date/time column (#17)
// ----------------------------------------------------------------------

/// The time after the date of a displayed date/time; letters are digits.
const TIME_SUFFIX: &str = " HH:MM:SS.mmm";
/// Digits of a timestamp in chronological order: `YYYYMMDDHHMMSSmmm`.
const CHRONO_DIGITS: usize = 17;
/// Position of every chronological digit in "YYYY-MM-DD HH:MM:SS.mmm".
const ISO_POS: [usize; CHRONO_DIGITS] =
    [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22];

/// One character of a displayed date/time: a literal or a digit of the timestamp.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Slot {
    Lit(char),
    /// Index into the chronological digits.
    Digit(u8),
}

/// How a date/time is displayed: the date pattern of the language (e.g. `{d}.{m}.{y}`) followed
/// by the time, character by character.
#[derive(Clone, Debug)]
pub struct DateLayout {
    slots: Vec<Slot>,
}

impl DateLayout {
    pub fn new(pattern: &str) -> DateLayout {
        let mut slots = Vec::new();
        let mut rest = pattern;
        while let Some(c) = rest.chars().next() {
            let field = [("{y}", 0u8, 4u8), ("{m}", 4, 2), ("{d}", 6, 2)]
                .into_iter()
                .find(|(token, _, _)| rest.starts_with(token));
            match field {
                Some((token, first, n)) => {
                    slots.extend((first..first + n).map(Slot::Digit));
                    rest = &rest[token.len()..];
                }
                None => {
                    slots.push(Slot::Lit(c));
                    rest = &rest[c.len_utf8()..];
                }
            }
        }
        let mut digit = 8u8;
        for c in TIME_SUFFIX.chars() {
            if c.is_ascii_alphabetic() {
                slots.push(Slot::Digit(digit));
                digit += 1;
            } else {
                slots.push(Slot::Lit(c));
            }
        }
        DateLayout { slots }
    }

    /// An ISO date-time ("2026-10-23 13:22:01.123") as displayed; other texts unchanged.
    pub fn format_iso(&self, iso: &str) -> String {
        let b = iso.as_bytes();
        if b.len() >= 10 && b[4] == b'-' && b[7] == b'-' {
            let date = &self.slots[..self.slots.len() - TIME_SUFFIX.len()];
            let mut out = String::with_capacity(date.len() + iso.len());
            for s in date {
                match *s {
                    Slot::Lit(c) => out.push(c),
                    Slot::Digit(k) => out.push(b[ISO_POS[k as usize]] as char),
                }
            }
            out.push_str(&iso[10..]);
            out
        } else {
            iso.to_owned()
        }
    }

    /// Calls `f` with a timestamp as displayed, written into a buffer on the stack.
    fn with_displayed<R>(&self, ms: i64, f: impl FnOnce(&str) -> R) -> R {
        let mut buf = [0u8; 128];
        let mut len = 0;
        let iso = match ms_to_iso_bytes(ms) {
            Ok(iso) => iso,
            Err(_) => return f(&self.format_iso(&ms_to_iso(ms))),
        };
        for s in &self.slots {
            match *s {
                Slot::Digit(k) if len < buf.len() => {
                    buf[len] = iso[ISO_POS[k as usize]];
                    len += 1;
                }
                Slot::Lit(c) if len + c.len_utf8() <= buf.len() => {
                    len += c.encode_utf8(&mut buf[len..]).len();
                }
                // A (very) long date pattern: build the text the usual way.
                _ => return f(&self.format_iso(&ms_to_iso(ms))),
            }
        }
        // Only whole characters were written.
        f(std::str::from_utf8(&buf[..len]).unwrap_or_default())
    }
}

/// A filter of the date/time column with the same meaning as a `ColumnFilter` on the displayed
/// text. Alternatives that select time ranges (a part of the displayed date/time that fixes the
/// digits from the year on, e.g. `23.10.2026 10:3` or `2026-10`) are compared with the stored
/// timestamp – no text per row. Other inputs (e.g. `:30`) match the displayed text, written into
/// a buffer on the stack; entries without a parsed timestamp match their date text.
#[derive(Clone)]
pub struct DateFilter {
    text: ColumnFilter,
    /// Inclusive ms ranges whose union matches; `None` = the displayed text has to be matched.
    ranges: Option<Vec<(i64, i64)>>,
    layout: DateLayout,
}

impl DateFilter {
    /// `None` when the text filters nothing (like `ColumnFilter::parse`).
    pub fn parse(input: &str, date_pattern: &str) -> Option<DateFilter> {
        let text = ColumnFilter::parse(input)?;
        let layout = DateLayout::new(date_pattern);
        let (_, alternatives) = split(input);
        let ranges = alternatives
            .iter()
            .map(|a| alternative_ranges(&layout, a))
            .collect::<Option<Vec<_>>>()
            .map(|r| r.concat());
        Some(DateFilter {
            text,
            ranges,
            layout,
        })
    }

    /// The filter matches the date/time of the entry as displayed.
    pub fn matches(&self, e: &EntryRef) -> bool {
        match (e.timestamp(), &self.ranges) {
            (Some(ts), Some(ranges)) => {
                ranges.iter().any(|&(lo, hi)| lo <= ts && ts <= hi) != self.text.negate
            }
            (Some(ts), None) => self.layout.with_displayed(ts, |s| self.text.matches(s)),
            (None, _) => self.text.matches(&self.layout.format_iso(&e.datetime())),
        }
    }

    /// The timestamps are compared as numbers (no text per row).
    #[cfg(test)]
    fn numeric(&self) -> bool {
        self.ranges.is_some()
    }
}

/// The timestamps whose displayed text contains (`=`: equals) the alternative, as inclusive ms
/// ranges; `None` when that set cannot be described by ranges here.
fn alternative_ranges(layout: &DateLayout, a: &Alternative) -> Option<Vec<(i64, i64)>> {
    let pat: Vec<char> = a.text.chars().collect();
    // Unicode case folding could turn other characters into digits or separators.
    if pat.iter().any(|c| !c.is_ascii()) {
        return None;
    }
    let n = layout.slots.len();
    let offsets = match a.exact {
        true if pat.len() == n => 0..1,
        false if pat.len() <= n => 0..n - pat.len() + 1,
        _ => 0..0,
    };
    let mut ranges = Vec::new();
    for off in offsets {
        let mut fixed = [None::<u8>; CHRONO_DIGITS];
        let fits = pat
            .iter()
            .zip(&layout.slots[off..])
            .all(|(&c, s)| match (*s, c.to_digit(10)) {
                (Slot::Lit(l), _) => l.eq_ignore_ascii_case(&c),
                (Slot::Digit(k), Some(d)) => {
                    let slot = &mut fixed[k as usize];
                    let same = slot.is_none_or(|x| u32::from(x) == d);
                    *slot = Some(d as u8);
                    same
                }
                (Slot::Digit(_), None) => false,
            });
        if fits {
            ranges.extend(prefix_range(&fixed)?);
        }
    }
    Some(ranges)
}

/// The timestamps whose chronological digits start with the fixed digits. `None` when the fixed
/// digits are not a prefix (e.g. day and month without the year: not one range); `Some(None)`
/// when no valid date/time has them.
fn prefix_range(fixed: &[Option<u8>; CHRONO_DIGITS]) -> Option<Option<(i64, i64)>> {
    let len = fixed.iter().take_while(|d| d.is_some()).count();
    if fixed[len..].iter().any(Option::is_some) {
        return None;
    }
    let lo: [u32; CHRONO_DIGITS] = std::array::from_fn(|i| fixed[i].map_or(0, u32::from));
    let hi: [u32; CHRONO_DIGITS] = std::array::from_fn(|i| fixed[i].map_or(9, u32::from));
    let field = |d: &[u32; CHRONO_DIGITS], r: std::ops::Range<usize>| {
        d[r].iter().fold(0, |v, &x| v * 10 + x)
    };
    let ms = |f: [u32; 7]| {
        iso_to_ms(&format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            f[0], f[1], f[2], f[3], f[4], f[5], f[6]
        ))
    };
    let fields = |d: &[u32; CHRONO_DIGITS]| {
        [0..4, 4..6, 6..8, 8..10, 10..12, 12..14, 14..17].map(|r| field(d, r))
    };
    // Lowest: zeros, month and day at least 1 (the fixed digits stay a prefix). When that is
    // not a valid date/time, the fixed digits make every candidate invalid.
    let mut low = fields(&lo);
    low[1] = low[1].max(1);
    low[2] = low[2].max(1);
    let Some(from) = ms(low) else {
        return Some(None);
    };
    // Highest: nines, every field clamped to its maximum (a clamped field keeps the prefix).
    let mut high = fields(&hi);
    high[1] = high[1].min(12);
    let month_days = (28..=31)
        .rev()
        .find(|&d| ms([high[0], high[1], d, 0, 0, 0, 0]).is_some())
        .unwrap_or(28);
    high[2] = high[2].min(month_days);
    high[3] = high[3].min(23);
    high[4] = high[4].min(59);
    high[5] = high[5].min(59);
    Some(ms(high).map(|to| (from, to)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(filter: &str, value: &str) -> bool {
        ColumnFilter::parse(filter).unwrap().matches(value)
    }

    #[test]
    fn syntax() {
        assert!(ColumnFilter::parse("").is_none());
        assert!(ColumnFilter::parse("  ! ").is_none());
        assert!(ColumnFilter::parse("| =").is_none());
        // contains, case-insensitive
        assert!(m("pack", "DISM Package Manager"));
        assert!(!m("pack", "DISM Provider Store"));
        // alternatives with spaces around them
        assert!(m("error | warn", "Warning"));
        assert!(!m("error|warn", "Info"));
        // exact match
        assert!(m("=info", "Info"));
        assert!(!m("=info", "Information"));
        assert!(m("=500|=404", "404"));
        assert!(!m("=500|=404", "4040"));
        // negation applies to all alternatives
        assert!(m("!dism", "CBS"));
        assert!(!m("!dism|cbs", "CBS"));
        assert!(m("!=info", "Information"));
        // special characters are literal
        assert!(m("a.b(", "xa.b(y"));
        assert!(!m("a.b", "axb"));
    }

    /// Log with entries around the given times plus a few without a parsed timestamp.
    fn dated_store() -> crate::store::LogStore {
        let base = iso_to_ms("2026-10-23 10:30:15.250").unwrap();
        let mut times: Vec<i64> = Vec::new();
        // Dense around the base time, sparse over a few years (leap day and year ends included).
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..4000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            times.push(base + (x % 7_200_000) as i64 - 3_600_000);
            times.push(base + (x % 4_000_000_000_000) as i64 - 2_000_000_000_000);
        }
        for iso in [
            "2024-02-29 23:59:59.999",
            "2026-12-31 23:59:59.999",
            "2027-01-01 00:00:00.000",
            "2026-10-23 00:00:00.000",
            "2026-10-23 23:59:59.999",
            "2026-10-24 00:00:00.000",
        ] {
            times.push(iso_to_ms(iso).unwrap());
        }
        let mut entries: Vec<crate::parser::LogEntry> = times
            .iter()
            .map(|&t| ms_to_iso(t))
            .chain(["".to_owned(), "23.10.2026 garbage".to_owned()])
            .map(|datetime| crate::parser::LogEntry {
                line_no: 1,
                message: String::new(),
                component: String::new(),
                datetime,
                thread: String::new(),
                severity: crate::parser::Severity::Info,
                extra: Vec::new(),
                raw: (0, 0),
            })
            .collect();
        entries.sort_by(|a, b| a.datetime.cmp(&b.datetime));
        crate::store::LogStore::from_entries(entries)
    }

    #[test]
    fn date_filter_equals_text_filter() {
        let store = dated_store();
        let cases: [(&str, &[(&str, bool)]); 2] = [
            (
                "{d}.{m}.{y}",
                &[
                    ("23.10.2026", true),
                    ("23.10.2026 10", true),
                    ("23.10.2026 10:3", true),
                    ("23.10.2026 10:30", true),
                    ("23.10.2026 10:30:1", true),
                    ("23.10.2026 10:30:15", true),
                    ("23.10.2026 10:30:15.2", true),
                    ("=23.10.2026 10:30:15.250", true),
                    ("=23.10.2026", true),
                    ("!23.10.2026", true),
                    ("!=23.10.2026 10:30:15.250", true),
                    ("23.10.2026|24.10.2026", true),
                    ("23.10.2026 10:2|23.10.2026 10:4", true),
                    ("!23.10.2026 10|23.10.2026 11", true),
                    ("2026", true),
                    (".2026", true),
                    ("10.2026", true),
                    ("2026 1", false),
                    ("29.02.2024", true),
                    ("30.02.2024", true),
                    ("31.04.2026", true),
                    ("23.10.2026 24", true),
                    ("23.10.2026 10:6", true),
                    ("garbage", true),
                    ("23.10.2026 garbage", true),
                    // Not ranges: matched as text.
                    ("3.10.2026", false),
                    (":30", false),
                    ("10:30", false),
                    ("23.10", false),
                    ("!:15", false),
                    ("2026|:30", false),
                ],
            ),
            (
                "{y}-{m}-{d}",
                &[
                    ("2026", true),
                    ("2026-1", true),
                    ("2026-10", true),
                    ("2026-10-2", true),
                    ("2026-10-23", true),
                    ("2026-10-23 10:30", true),
                    ("2026-02-3", true),
                    ("2026-13", true),
                    ("2024-02-29", true),
                    ("=2026-10-23 10:30:15.250", true),
                    ("!2026-10", true),
                    ("026-10", false),
                    ("-10-23", false),
                    ("10:30:15", false),
                ],
            ),
        ];
        for (pattern, inputs) in cases {
            let layout = DateLayout::new(pattern);
            for &(input, numeric) in inputs {
                let f = DateFilter::parse(input, pattern).unwrap();
                assert_eq!(f.numeric(), numeric, "{pattern} {input:?}");
                let text = ColumnFilter::parse(input).unwrap();
                for i in 0..store.len() {
                    let e = store.at(i);
                    let expected = text.matches(&layout.format_iso(&e.datetime()));
                    assert_eq!(
                        f.matches(&e),
                        expected,
                        "{pattern} {input:?} {}",
                        e.datetime()
                    );
                }
            }
        }
    }

    #[test]
    fn date_layout() {
        let sk = DateLayout::new("{d}.{m}.{y}");
        assert_eq!(
            sk.format_iso("2026-10-23 13:22:01.123"),
            "23.10.2026 13:22:01.123"
        );
        assert_eq!(sk.format_iso("2026-10-23"), "23.10.2026");
        assert_eq!(sk.format_iso("Oct 23"), "Oct 23");
        let ts = iso_to_ms("2026-10-23 13:22:01.123").unwrap();
        sk.with_displayed(ts, |s| assert_eq!(s, "23.10.2026 13:22:01.123"));
        let en = DateLayout::new("{y}-{m}-{d}");
        en.with_displayed(ts, |s| assert_eq!(s, "2026-10-23 13:22:01.123"));
        let odd = DateLayout::new("{d}. {m}. {y} ő");
        odd.with_displayed(ts, |s| assert_eq!(s, "23. 10. 2026 ő 13:22:01.123"));
        assert_eq!(
            odd.format_iso("2026-10-23 13:22:01.123"),
            "23. 10. 2026 ő 13:22:01.123"
        );
    }
}
