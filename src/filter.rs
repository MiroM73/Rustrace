//! Which entries are displayed and which match the search: search text, filter mode, severity
//! toggles, time range and column filters. Pure logic – the UI (`app/filter.rs`) runs it on the
//! UI thread for small logs and appended rows, and in a background thread for a full re-filter
//! of a big log (#16).

use crate::colfilter::{ColumnFilter, DateFilter};
use crate::logdef::{ColumnKind, Role};
use crate::parser::Severity;
use crate::store::{EntryRef, LogStore};
use regex::Regex;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Entries evaluated by one worker at a time (a multiple of 64, so the match bits of the chunks
/// can be joined word by word).
const CHUNK: usize = 64 * 1024;

// ----------------------------------------------------------------------
// Match bits (#18)
// ----------------------------------------------------------------------

/// One bit per entry: does it match the search? Keeps the number of set bits, so the match
/// count never needs a recount.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct MatchBits {
    words: Vec<u64>,
    len: usize,
    ones: usize,
}

impl MatchBits {
    pub fn len(&self) -> usize {
        self.len
    }

    /// Number of entries that match.
    pub fn count(&self) -> usize {
        self.ones
    }

    pub fn get(&self, i: usize) -> bool {
        i < self.len && self.words[i / 64] >> (i % 64) & 1 == 1
    }

    pub fn push(&mut self, b: bool) {
        if self.len.is_multiple_of(64) {
            self.words.push(0);
        }
        if b {
            self.words[self.len / 64] |= 1 << (self.len % 64);
            self.ones += 1;
        }
        self.len += 1;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Sets bit `i` (the set grows as needed, new bits are 0).
    pub fn set(&mut self, i: usize, b: bool) {
        if i >= self.len {
            if !b {
                return;
            }
            self.len = i + 1;
            self.words.resize(self.len.div_ceil(64), 0);
        }
        let (w, m) = (i / 64, 1u64 << (i % 64));
        if (self.words[w] & m != 0) != b {
            self.words[w] ^= m;
            if b {
                self.ones += 1;
            } else {
                self.ones -= 1;
            }
        }
    }

    /// Index of the first / last set bit.
    pub fn first_one(&self) -> Option<usize> {
        let w = self.words.iter().position(|&w| w != 0)?;
        Some(w * 64 + self.words[w].trailing_zeros() as usize)
    }

    pub fn last_one(&self) -> Option<usize> {
        let w = self.words.iter().rposition(|&w| w != 0)?;
        Some(w * 64 + 63 - self.words[w].leading_zeros() as usize)
    }

    /// Indexes of the set bits, in order.
    pub fn ones(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(w, &word)| {
            let mut word = word;
            std::iter::from_fn(move || {
                (word != 0).then(|| {
                    let b = word.trailing_zeros() as usize;
                    word &= word - 1;
                    w * 64 + b
                })
            })
        })
    }

    /// Keeps the first `n` bits; costs O(removed bits).
    pub fn truncate(&mut self, n: usize) {
        if n >= self.len {
            return;
        }
        self.ones -= self.count_from(n);
        self.words.truncate(n.div_ceil(64));
        if !n.is_multiple_of(64) {
            self.words[n / 64] &= (1 << (n % 64)) - 1;
        }
        self.len = n;
    }

    /// Removes the first `k` bits (the oldest entries dropped by the memory limit).
    pub fn drain_front(&mut self, k: usize) {
        let k = k.min(self.len);
        let (skip, shift) = (k / 64, k % 64);
        let words = &self.words[skip..];
        let shifted: Vec<u64> = (0..words.len())
            .map(|i| match shift {
                0 => words[i],
                _ => words[i] >> shift | words.get(i + 1).map_or(0, |w| w << (64 - shift)),
            })
            .collect();
        let len = self.len - k;
        self.words = shifted;
        self.words.truncate(len.div_ceil(64));
        self.len = len;
        self.ones = self.recount();
    }

    /// Set bits from `start` to the end.
    fn count_from(&self, start: usize) -> usize {
        if start >= self.len {
            return 0;
        }
        let first = self.words[start / 64] >> (start % 64);
        let rest: usize = self.words[start / 64 + 1..]
            .iter()
            .map(|w| w.count_ones() as usize)
            .sum();
        first.count_ones() as usize + rest
    }

    /// Full count of the set bits (the stored count must always equal it).
    pub fn recount(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Appends the bits of `other` (whose start must be word aligned: `self.len % 64 == 0`).
    fn append_aligned(&mut self, other: MatchBits) {
        debug_assert_eq!(self.len % 64, 0);
        self.words.extend(other.words);
        self.len += other.len;
        self.ones += other.ones;
    }
}

// ----------------------------------------------------------------------
// Filter
// ----------------------------------------------------------------------

/// The search matches the message, the component or an extra column.
pub fn entry_matches(re: &Regex, e: &EntryRef) -> bool {
    re.is_match(e.message) || other_columns_match(re, e)
}

/// Component or one of the extra columns of a log definition matches.
pub fn other_columns_match(re: &Regex, e: &EntryRef) -> bool {
    re.is_match(e.component) || e.extras().any(|v| re.is_match(v))
}

/// A compiled filter of one column.
#[derive(Clone)]
pub enum CellFilter {
    Text(ColumnFilter),
    /// The date/time column: compared with the timestamps where possible (#17).
    Date(DateFilter),
}

/// Everything that decides which entries are displayed and which match the search.
#[derive(Clone, Default)]
pub struct FilterSpec {
    pub search: Option<Regex>,
    /// Only the entries matching the search are displayed.
    pub filter_mode: bool,
    /// Info, warning, error.
    pub show_sev: [bool; 3],
    /// Inclusive ms bounds; with a bound, entries without a date/time are hidden.
    pub time_range: (Option<i64>, Option<i64>),
    pub columns: Vec<(ColumnKind, CellFilter)>,
    /// Labels of the File column (index = file id).
    pub file_labels: Vec<String>,
    /// Only bookmarked entries are displayed: their (file id, line number) (#22).
    pub bookmarks: Option<Arc<HashSet<(u16, usize)>>>,
}

/// Result of filtering entries: indexes of the displayed ones and the search match bits.
#[derive(Default, Debug, PartialEq)]
pub struct FilterOutput {
    pub filtered: Vec<usize>,
    pub is_match: MatchBits,
}

impl FilterSpec {
    /// (matches the search, is displayed) for one entry.
    pub fn eval(&self, e: &EntryRef) -> (bool, bool) {
        let m = self.search.as_ref().is_some_and(|re| entry_matches(re, e));
        let pass_search = !self.filter_mode || self.search.is_none() || m;
        let shown = pass_search
            && self.sev_visible(e.severity)
            && self.in_time_range(e.timestamp())
            && self
                .bookmarks
                .as_ref()
                .is_none_or(|b| b.contains(&(e.file, e.line_no)))
            && self.passes_columns(e);
        (m, shown)
    }

    fn sev_visible(&self, s: Severity) -> bool {
        match s {
            Severity::Info => self.show_sev[0],
            Severity::Warning => self.show_sev[1],
            Severity::Error => self.show_sev[2],
        }
    }

    fn in_time_range(&self, ts: Option<i64>) -> bool {
        match self.time_range {
            (None, None) => true,
            (from, to) => {
                ts.is_some_and(|t| from.is_none_or(|f| t >= f) && to.is_none_or(|x| t <= x))
            }
        }
    }

    /// All column filters match the entry (the date/time as displayed).
    fn passes_columns(&self, e: &EntryRef) -> bool {
        self.columns.iter().all(|(kind, f)| {
            let f = match f {
                CellFilter::Date(d) => return d.matches(e),
                CellFilter::Text(f) => f,
            };
            match kind {
                ColumnKind::Role(Role::Message) => f.matches(e.message),
                ColumnKind::Role(Role::Component) => f.matches(e.component),
                ColumnKind::Role(Role::Thread) => f.matches(e.thread),
                // The UI builds `CellFilter::Date` for this column; a text filter sees ISO text.
                ColumnKind::Role(Role::DateTime) => f.matches(&e.datetime()),
                ColumnKind::Extra(i) => f.matches(e.extra(*i)),
                ColumnKind::File => f.matches(
                    self.file_labels
                        .get(e.file as usize)
                        .map_or("", String::as_str),
                ),
            }
        })
    }

    /// Filters the entries `range` on this thread, appending to `out`.
    pub fn run(&self, store: &LogStore, range: std::ops::Range<usize>, out: &mut FilterOutput) {
        for i in range {
            let (m, shown) = self.eval(&store.at(i));
            out.is_match.push(m);
            if shown {
                out.filtered.push(i);
            }
        }
    }

    /// Filters all entries in chunks on all CPU cores; `None` when cancelled. The result is the
    /// same as `run` over all entries.
    pub fn run_parallel(&self, store: &LogStore, cancel: &AtomicBool) -> Option<FilterOutput> {
        let n = store.len();
        let chunks = n.div_ceil(CHUNK);
        // One core stays free for the UI thread, so it keeps drawing frames on time.
        let threads = std::thread::available_parallelism()
            .map_or(1, |p| p.get().saturating_sub(1))
            .min(chunks)
            .max(1);
        let next = AtomicUsize::new(0);
        let parts: Mutex<Vec<Option<FilterOutput>>> =
            Mutex::new((0..chunks).map(|_| None).collect());
        let work = || {
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let c = next.fetch_add(1, Ordering::Relaxed);
                if c >= chunks {
                    return;
                }
                let mut out = FilterOutput::default();
                self.run(store, c * CHUNK..((c + 1) * CHUNK).min(n), &mut out);
                parts.lock().unwrap_or_else(|e| e.into_inner())[c] = Some(out);
            }
        };
        std::thread::scope(|s| {
            for _ in 1..threads {
                s.spawn(work);
            }
            work();
        });
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let parts = parts.into_inner().unwrap_or_else(|e| e.into_inner());
        let mut out = FilterOutput {
            filtered: Vec::with_capacity(parts.iter().flatten().map(|p| p.filtered.len()).sum()),
            is_match: MatchBits::default(),
        };
        for p in parts {
            // A worker that panicked left its chunk empty: treat as cancelled.
            let p = p?;
            out.filtered.extend(p.filtered);
            out.is_match.append_aligned(p.is_match);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::LogEntry;

    fn bits(v: &[bool]) -> MatchBits {
        let mut b = MatchBits::default();
        v.iter().for_each(|&x| b.push(x));
        b
    }

    fn pattern(n: usize) -> Vec<bool> {
        (0..n).map(|i| i % 3 == 0 || i % 7 == 1).collect()
    }

    #[test]
    fn match_bits_keep_the_count() {
        for n in [0, 1, 63, 64, 65, 200, 1000] {
            let v = pattern(n);
            let b = bits(&v);
            assert_eq!(b.len(), n);
            assert_eq!(b.count(), v.iter().filter(|&&x| x).count());
            assert!((0..n + 3).all(|i| b.get(i) == v.get(i).copied().unwrap_or(false)));
            for cut in [0, 1, 5, 63, 64, 65, 128, 999, 1000, 2000] {
                // truncate + append = the incremental re-filter after an auto refresh
                let mut t = b.clone();
                t.truncate(cut);
                let kept = &v[..cut.min(n)];
                assert_eq!(t, bits(kept), "truncate {n} {cut}");
                assert_eq!(t.count(), t.recount());
                for x in [true, false, true] {
                    t.push(x);
                }
                assert_eq!(t.count(), t.recount());
                // drain_front = drop_front of the memory limit
                let mut d = b.clone();
                d.drain_front(cut);
                assert_eq!(d, bits(&v[cut.min(n)..]), "drain {n} {cut}");
                assert_eq!(d.count(), d.recount());
            }
        }
    }

    #[test]
    fn match_bits_set_and_iterate() {
        let mut b = MatchBits::default();
        assert_eq!((b.first_one(), b.last_one()), (None, None));
        b.set(5, false);
        assert_eq!(b.len(), 0);
        for i in [3, 64, 200, 63] {
            b.set(i, true);
        }
        b.set(64, true);
        assert_eq!(b.count(), 4);
        assert_eq!(b.ones().collect::<Vec<_>>(), [3, 63, 64, 200]);
        assert_eq!((b.first_one(), b.last_one()), (Some(3), Some(200)));
        b.set(3, false);
        b.set(200, false);
        assert_eq!(b.count(), b.recount());
        assert_eq!(b.ones().collect::<Vec<_>>(), [63, 64]);
        assert_eq!((b.first_one(), b.last_one()), (Some(63), Some(64)));
        b.drain_front(63);
        assert_eq!(b.ones().collect::<Vec<_>>(), [0, 1]);
    }

    fn store(n: usize) -> LogStore {
        LogStore::from_entries((0..n).map(|i| LogEntry {
            line_no: i + 1,
            message: format!(
                "message {i} {}",
                if i % 5 == 0 { "error 0x80070005" } else { "ok" }
            ),
            component: ["Setup", "Agent", "Downloader"][i % 3].to_owned(),
            datetime: format!("2026-10-23 10:{:02}:{:02}.000", i / 60 % 60, i % 60),
            thread: (i % 4).to_string(),
            severity: [Severity::Info, Severity::Warning, Severity::Error][i % 7 % 3],
            extra: Vec::new(),
            raw: (0, 0),
        }))
    }

    #[test]
    fn parallel_equals_sequential() {
        let s = store(3 * CHUNK + 1234);
        let date = |t: &str| CellFilter::Date(DateFilter::parse(t, "{d}.{m}.{y}").unwrap());
        let text = |t: &str| CellFilter::Text(ColumnFilter::parse(t).unwrap());
        let specs = [
            FilterSpec {
                show_sev: [true; 3],
                ..Default::default()
            },
            FilterSpec {
                search: Some(Regex::new("(?i)ERROR 0x8007").unwrap()),
                show_sev: [true, false, true],
                ..Default::default()
            },
            FilterSpec {
                search: Some(Regex::new("agent").unwrap()),
                filter_mode: true,
                show_sev: [true; 3],
                time_range: (Some(0), None),
                columns: vec![
                    (ColumnKind::Role(Role::DateTime), date("23.10.2026 10:1|:3")),
                    (ColumnKind::Role(Role::Thread), text("!=2")),
                ],
                ..Default::default()
            },
            FilterSpec {
                show_sev: [true; 3],
                bookmarks: Some(Arc::new([(0, 1), (0, 5000), (0, 3 * CHUNK)].into())),
                ..Default::default()
            },
        ];
        for spec in specs {
            let mut seq = FilterOutput::default();
            spec.run(&s, 0..s.len(), &mut seq);
            let par = spec.run_parallel(&s, &AtomicBool::new(false)).unwrap();
            assert_eq!(par, seq);
            if let Some(b) = &spec.bookmarks {
                // Only the bookmarked lines (line numbers are 1-based).
                assert_eq!(seq.filtered, [0, 4999, 3 * CHUNK - 1]);
                assert_eq!(b.len(), 3);
            }
            assert_eq!(par.is_match.count(), par.is_match.recount());
        }
        assert!(
            FilterSpec::default()
                .run_parallel(&s, &AtomicBool::new(true))
                .is_none()
        );
        let empty = FilterSpec::default().run_parallel(&LogStore::new(), &AtomicBool::new(false));
        assert_eq!(empty, Some(FilterOutput::default()));
    }

    /// `RUSTRACE_BENCH_FILE=<log> cargo test --release bench_filter -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_filter() {
        use crate::parser::{LogSource, PollResult, Progress};
        let path = std::env::var("RUSTRACE_BENCH_FILE").expect("RUSTRACE_BENCH_FILE");
        let Ok(PollResult::Reset { committed: s, .. }) =
            LogSource::new(&path, true).poll(&Progress::default())
        else {
            panic!()
        };
        let pattern = "{d}.{m}.{y}";
        let day = crate::store::ms_to_iso(s.timestamp(s.len() / 2).expect("dated log"));
        let day = crate::colfilter::DateLayout::new(pattern).format_iso(&day[..10]);
        let time = |f: &dyn Fn() -> usize| {
            let t = std::time::Instant::now();
            let n = f();
            (t.elapsed(), n)
        };
        // Before #17: the date of every row formatted as text (`App::display_dt` of v0.7.0).
        let display_dt = |iso: &str| {
            let date = pattern
                .replace("{y}", &iso[0..4])
                .replace("{m}", &iso[5..7])
                .replace("{d}", &iso[8..10]);
            format!("{date}{}", &iso[10..])
        };
        let text = ColumnFilter::parse(&day).unwrap();
        let old = time(&|| {
            (0..s.len())
                .filter(|&i| text.matches(&display_dt(&s.datetime(i))))
                .count()
        });
        let spec = |input: &str| FilterSpec {
            show_sev: [true; 3],
            columns: vec![(
                ColumnKind::Role(Role::DateTime),
                CellFilter::Date(DateFilter::parse(input, pattern).unwrap()),
            )],
            ..Default::default()
        };
        let count = |spec: &FilterSpec| {
            let mut out = FilterOutput::default();
            spec.run(&s, 0..s.len(), &mut out);
            out.filtered.len()
        };
        let range = spec(&day);
        let new = time(&|| count(&range));
        let fallback = spec(":30");
        let new_text = time(&|| count(&fallback));
        let search = FilterSpec {
            search: Some(Regex::new("(?i)error 0x8007").unwrap()),
            show_sev: [true; 3],
            ..Default::default()
        };
        let seq = time(&|| {
            let mut out = FilterOutput::default();
            search.run(&s, 0..s.len(), &mut out);
            out.is_match.count()
        });
        let par = time(&|| {
            search
                .run_parallel(&s, &AtomicBool::new(false))
                .unwrap()
                .is_match
                .count()
        });
        println!(
            "BENCH entries={} date filter {day:?}: text {:.2?} ({}) → range {:.2?} ({}); \
             \":30\" on stack {:.2?} ({}); search sequential {:.2?} ({}) → parallel {:.2?} ({})",
            s.len(),
            old.0,
            old.1,
            new.0,
            new.1,
            new_text.0,
            new_text.1,
            seq.0,
            seq.1,
            par.0,
            par.1
        );
    }
}
