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

// ----------------------------------------------------------------------
// Matches of the interned values (#33)
// ----------------------------------------------------------------------

/// One bit per interned string of a store: does it match? Components, threads and extra columns
/// repeat a lot (a few hundred distinct components in 1.9 M entries), so a full run evaluates
/// the search / a column filter once per distinct value and only looks the bit up per entry.
struct IdBits(Vec<u64>);

impl IdBits {
    fn get(&self, id: u32) -> bool {
        let i = id as usize;
        self.0.get(i / 64).is_some_and(|w| w >> (i % 64) & 1 == 1)
    }

    /// `pred` of every interned string of `store`, on `threads` threads.
    fn build(store: &LogStore, threads: usize, pred: &(dyn Fn(&str) -> bool + Sync)) -> IdBits {
        let n = store.strings_len();
        let mut words = vec![0u64; n.div_ceil(64)];
        let per = words.len().div_ceil(threads.max(1)).max(1);
        let fill = |first_word: usize, chunk: &mut [u64]| {
            for (w, word) in chunk.iter_mut().enumerate() {
                let base = (first_word + w) * 64;
                for b in 0..64.min(n - base) {
                    if pred(store.string((base + b) as u32)) {
                        *word |= 1 << b;
                    }
                }
            }
        };
        std::thread::scope(|s| {
            let mut chunks = words.chunks_mut(per).enumerate();
            let first = chunks.next();
            for (k, chunk) in chunks {
                s.spawn(move || fill(k * per, chunk));
            }
            if let Some((_, chunk)) = first {
                fill(0, chunk);
            }
        });
        IdBits(words)
    }
}

/// The interned-value tables of one filter run: the search and, per column filter of a
/// component / thread / extra column, its own (`None` = evaluated per entry).
struct IdTables {
    search: Option<IdBits>,
    columns: Vec<Option<IdBits>>,
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

/// The search as entered: text, regular expression, case-sensitive (#33).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SearchKey {
    pub text: String,
    pub regex: bool,
    pub case: bool,
}

impl SearchKey {
    /// Every entry that matches this search also matches `prev`: both are plain text with the
    /// same case sensitivity and `prev` is a part of this text (`acce` → `acces`), so only the
    /// matches of `prev` need to be searched again (`FilterSpec::narrow`).
    pub fn narrows(&self, prev: &SearchKey) -> bool {
        !self.regex
            && !prev.regex
            && self.case == prev.case
            && !prev.text.is_empty()
            && self.text != prev.text
            && self.text.contains(prev.text.as_str())
    }
}

/// The match bits of the last few searches (#33), for going back to an earlier search text
/// (Backspace after a typo, the search history): valid only for the same data (`data_gen`) and
/// entry count.
#[derive(Default)]
pub struct SearchCache {
    /// (search, data generation, bits), the newest last.
    entries: Vec<(SearchKey, u64, MatchBits)>,
}

impl SearchCache {
    /// Searches kept (a `MatchBits` of 1.9 M entries is ~240 kB).
    pub const SIZE: usize = 5;

    pub fn get(&self, key: &SearchKey, data_gen: u64, len: usize) -> Option<&MatchBits> {
        self.entries
            .iter()
            .find(|(k, g, b)| k == key && *g == data_gen && b.len() == len)
            .map(|(_, _, b)| b)
    }

    /// Keeps the bits of `key`; entries of older data are dropped, the oldest beyond `SIZE`.
    pub fn put(&mut self, key: &SearchKey, data_gen: u64, bits: &MatchBits) {
        self.entries.retain(|(k, g, _)| *g == data_gen && k != key);
        if self.entries.len() >= Self::SIZE {
            self.entries.remove(0);
        }
        self.entries.push((key.clone(), data_gen, bits.clone()));
    }
}

/// Result of filtering entries: indexes of the displayed ones and the search match bits.
#[derive(Default, Debug, PartialEq)]
pub struct FilterOutput {
    /// Entry indexes as `u32` (#33): half the memory of `usize` for millions of rows.
    pub filtered: Vec<u32>,
    pub is_match: MatchBits,
}

impl FilterSpec {
    /// (matches the search, is displayed) for one entry; `t` = the interned-value tables of the
    /// run (the same result as without them, #33).
    fn eval_with(&self, e: &EntryRef, t: Option<&IdTables>) -> (bool, bool) {
        let m = self
            .search
            .as_ref()
            .is_some_and(|re| match t.and_then(|t| t.search.as_ref()) {
                Some(bits) => {
                    re.is_match(e.message)
                        || bits.get(e.component_id())
                        || e.extra_ids().any(|id| bits.get(id))
                }
                None => entry_matches(re, e),
            });
        let pass_search = !self.filter_mode || self.search.is_none() || m;
        let shown = pass_search
            && self.sev_visible(e.severity)
            && self.in_time_range(e.timestamp())
            && self
                .bookmarks
                .as_ref()
                .is_none_or(|b| b.contains(&(e.file, e.line_no)))
            && self.passes_columns(e, t);
        (m, shown)
    }

    /// The interned id of a column whose values are interned (component, thread, extra).
    fn interned_id(kind: &ColumnKind, e: &EntryRef) -> Option<u32> {
        match kind {
            ColumnKind::Role(Role::Component) => Some(e.component_id()),
            ColumnKind::Role(Role::Thread) => Some(e.thread_id()),
            ColumnKind::Extra(i) => Some(e.extra_id(*i)),
            _ => None,
        }
    }

    /// The tables for a run over `rows` entries of `store`, when they pay off: building one
    /// costs one evaluation per interned string, so only when there are at least as many rows
    /// (a full re-filter; not the few rows appended by an auto refresh).
    fn id_tables(&self, store: &LogStore, rows: usize, threads: usize) -> Option<IdTables> {
        let columns_interned = self.columns.iter().any(|(k, f)| {
            matches!(f, CellFilter::Text(_))
                && matches!(
                    k,
                    ColumnKind::Role(Role::Component | Role::Thread) | ColumnKind::Extra(_)
                )
        });
        if (self.search.is_none() && !columns_interned) || rows < store.strings_len() {
            return None;
        }
        Some(self.build_tables(store, threads))
    }

    fn build_tables(&self, store: &LogStore, threads: usize) -> IdTables {
        IdTables {
            search: self
                .search
                .as_ref()
                .map(|re| IdBits::build(store, threads, &|s| re.is_match(s))),
            columns: self
                .columns
                .iter()
                .map(|(kind, f)| match (kind, f) {
                    (
                        ColumnKind::Role(Role::Component | Role::Thread) | ColumnKind::Extra(_),
                        CellFilter::Text(f),
                    ) => Some(IdBits::build(store, threads, &|s| f.matches(s))),
                    _ => None,
                })
                .collect(),
        }
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
    fn passes_columns(&self, e: &EntryRef, t: Option<&IdTables>) -> bool {
        self.columns.iter().enumerate().all(|(j, (kind, f))| {
            if let Some(bits) = t.and_then(|t| t.columns.get(j)?.as_ref())
                && let Some(id) = Self::interned_id(kind, e)
            {
                return bits.get(id);
            }
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
                // Computed for display only, never filtered.
                ColumnKind::Delta => true,
                ColumnKind::File => f.matches(
                    self.file_labels
                        .get(e.file as usize)
                        .map_or("", String::as_str),
                ),
            }
        })
    }

    /// The result for a narrowed search (#33): `prev` is the result of this spec with a search
    /// that every match of this one also matches (`SearchKey::narrows`), over all entries of
    /// `store`. Only the previous matches are searched again; the result equals a full `run`.
    pub fn narrow(&self, store: &LogStore, prev: &FilterOutput) -> FilterOutput {
        let Some(re) = &self.search else {
            let mut out = FilterOutput::default();
            self.run(store, 0..store.len(), &mut out);
            return out;
        };
        let mut is_match = prev.is_match.clone();
        for i in prev.is_match.ones() {
            if !entry_matches(re, &store.at(i)) {
                is_match.set(i, false);
            }
        }
        // Without the filter mode the displayed entries do not depend on the search.
        let filtered = match self.filter_mode {
            true => prev
                .filtered
                .iter()
                .copied()
                .filter(|&i| is_match.get(i as usize))
                .collect(),
            false => prev.filtered.clone(),
        };
        FilterOutput { filtered, is_match }
    }

    /// Filters the entries `range` on this thread, appending to `out`.
    pub fn run(&self, store: &LogStore, range: std::ops::Range<usize>, out: &mut FilterOutput) {
        let tables = self.id_tables(store, range.len(), 1);
        self.run_with(store, range, out, tables.as_ref());
    }

    fn run_with(
        &self,
        store: &LogStore,
        range: std::ops::Range<usize>,
        out: &mut FilterOutput,
        tables: Option<&IdTables>,
    ) {
        for i in range {
            let (m, shown) = self.eval_with(&store.at(i), tables);
            out.is_match.push(m);
            if shown {
                // `filtered` holds u32 indexes (#33); the memory limit keeps a log far below.
                out.filtered
                    .push(u32::try_from(i).expect("more than 4 294 967 295 entries"));
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
        let tables = self.id_tables(store, n, threads);
        let tables = tables.as_ref();
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
                self.run_with(store, c * CHUNK..((c + 1) * CHUNK).min(n), &mut out, tables);
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

    /// A store with repeating components, threads and extra columns (some entries without the
    /// later extra columns), for the interned-value tables (#33).
    fn random_store(seed: u64, n: usize) -> LogStore {
        let mut x = seed | 1;
        let mut next = move |m: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % m
        };
        let words = [
            "ccm", "CcmExec", "dism", "Error", "warn", "abc", "x1", "", "42", "-7",
        ];
        (0..n)
            .map(|i| {
                let n_extra = next(4);
                let mut w = || words[next(words.len() as u64) as usize].to_owned();
                let extras = (0..n_extra).map(|_| w()).collect();
                LogEntry {
                    line_no: i + 1,
                    message: format!("{} {}", w(), w()),
                    component: w(),
                    datetime: String::new(),
                    thread: w(),
                    severity: Severity::Info,
                    extra: extras,
                    raw: (0, 0),
                }
            })
            .fold(LogStore::new(), |mut s, e| {
                s.push(e, 0);
                s
            })
    }

    /// A narrowed search gives the same result as a full search (#33).
    #[test]
    fn narrowed_search_equals_full_search() {
        let store = random_store(4242, 4000);
        let key = |text: &str| SearchKey {
            text: text.into(),
            regex: false,
            case: false,
        };
        let steps = [
            ("c", "cc"),
            ("cc", "ccm"),
            ("ccm", "ccmexec"),
            ("e", "err"),
            ("err", "error"),
            ("r", "warn"),
            ("4", "42"),
            ("x", "x1"),
            ("a", "abc"),
        ];
        for (old, new) in steps {
            assert!(key(new).narrows(&key(old)), "{old} → {new}");
            for filter_mode in [false, true] {
                let spec = |text: &str| FilterSpec {
                    search: Some(Regex::new(&format!("(?i){}", regex::escape(text))).unwrap()),
                    filter_mode,
                    show_sev: [true; 3],
                    columns: vec![(
                        ColumnKind::Role(Role::Thread),
                        CellFilter::Text(ColumnFilter::parse("!=dism").unwrap()),
                    )],
                    ..Default::default()
                };
                let mut prev = FilterOutput::default();
                spec(old).run(&store, 0..store.len(), &mut prev);
                let mut full = FilterOutput::default();
                spec(new).run(&store, 0..store.len(), &mut full);
                let narrowed = spec(new).narrow(&store, &prev);
                assert_eq!(narrowed, full, "{old} → {new}, filter mode {filter_mode}");
                assert_eq!(narrowed.is_match.count(), narrowed.is_match.recount());
            }
        }
        // Not narrowing: shorter, changed, regex, other case, empty or the same text.
        let k = |text: &str, regex, case| SearchKey {
            text: text.into(),
            regex,
            case,
        };
        assert!(!key("cc").narrows(&key("ccm")));
        assert!(!key("ccx").narrows(&key("ccm")));
        assert!(!k("ccm", true, false).narrows(&key("cc")));
        assert!(!key("ccm").narrows(&k("cc", true, false)));
        assert!(!k("ccm", false, true).narrows(&key("cc")));
        assert!(!key("ccm").narrows(&key("")));
        assert!(!key("ccm").narrows(&key("ccm")));
        // A part in the middle also narrows ("rro" → "error").
        assert!(key("error").narrows(&key("rro")));
    }

    #[test]
    fn search_cache_keeps_the_last_searches_of_the_same_data() {
        let key = |t: &str| SearchKey {
            text: t.into(),
            regex: false,
            case: false,
        };
        let b = bits(&pattern(100));
        let mut c = SearchCache::default();
        for t in ["a", "b", "c", "d", "e"] {
            c.put(&key(t), 1, &b);
        }
        assert!(c.get(&key("a"), 1, 100).is_some());
        // A sixth search drops the oldest one; putting a known one again makes it the newest.
        c.put(&key("b"), 1, &b);
        c.put(&key("f"), 1, &b);
        assert!(c.get(&key("a"), 1, 100).is_none());
        assert!(c.get(&key("b"), 1, 100).is_some());
        assert_eq!(c.get(&key("f"), 1, 100), Some(&b));
        // Other data, entry count, case or regex flag: not found.
        assert!(c.get(&key("f"), 2, 100).is_none());
        assert!(c.get(&key("f"), 1, 101).is_none());
        let mut regex = key("f");
        regex.regex = true;
        assert!(c.get(&regex, 1, 100).is_none());
        // New data drops everything older.
        c.put(&key("g"), 2, &b);
        assert!(c.get(&key("f"), 1, 100).is_none());
        assert_eq!(c.entries.len(), 1);
    }

    #[test]
    fn interned_tables_give_the_same_result() {
        let searches = ["ccm", "error", "x", "^$", "4", "nothing"];
        let filters = [
            "ccm",
            "=dism",
            "!ccm&!=warn",
            ">0",
            "a|=x1",
            "!=abc",
            "e&!error",
        ];
        for seed in 1..6u64 {
            let store = random_store(seed * 7919, 3000);
            for (k, search) in searches.iter().enumerate() {
                let f = filters[k % filters.len()];
                let g = filters[(k + seed as usize) % filters.len()];
                let spec = FilterSpec {
                    search: Some(Regex::new(&format!("(?i){search}")).unwrap()),
                    filter_mode: k % 2 == 0,
                    show_sev: [true; 3],
                    columns: vec![
                        (
                            ColumnKind::Role(Role::Component),
                            CellFilter::Text(ColumnFilter::parse(f).unwrap()),
                        ),
                        (
                            ColumnKind::Extra(1),
                            CellFilter::Text(ColumnFilter::parse(g).unwrap()),
                        ),
                        (
                            ColumnKind::Role(Role::Thread),
                            CellFilter::Text(ColumnFilter::parse("!zzz").unwrap()),
                        ),
                    ],
                    ..Default::default()
                };
                let mut plain = FilterOutput::default();
                spec.run_with(&store, 0..store.len(), &mut plain, None);
                for threads in [1, 3] {
                    let tables = spec.build_tables(&store, threads);
                    let mut fast = FilterOutput::default();
                    spec.run_with(&store, 0..store.len(), &mut fast, Some(&tables));
                    assert_eq!(fast, plain, "seed {seed} search {search:?} {f:?} {g:?}");
                }
                // `run` uses the tables (3000 rows, a few interned strings), the result stays.
                let mut out = FilterOutput::default();
                spec.run(&store, 0..store.len(), &mut out);
                assert_eq!(out, plain);
                assert!(spec.id_tables(&store, store.len(), 1).is_some());
                assert!(spec.id_tables(&store, 3, 1).is_none());
                assert_eq!(
                    spec.run_parallel(&store, &AtomicBool::new(false)),
                    Some(plain)
                );
            }
        }
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
                assert_eq!(seq.filtered, [0, 4999, 3 * CHUNK as u32 - 1]);
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
    /// (the built-in log definitions apply, so IIS / CBS logs get their columns). Measures the
    /// date column filter (dated logs only), the search sequential / parallel and a search that
    /// hits the extra columns (`RUSTRACE_BENCH_SEARCH2`, default `jdoe`).
    #[test]
    #[ignore]
    fn bench_filter() {
        use crate::parser::{LogSource, PollResult, Progress};
        let path = std::env::var("RUSTRACE_BENCH_FILE").expect("RUSTRACE_BENCH_FILE");
        let mut src = LogSource::new(&path, true);
        src.defs = Arc::new(crate::logdef::load_from(&[], crate::logdef::SourceFilter::All).defs);
        let Ok(PollResult::Reset { committed: s, .. }) = src.poll(&Progress::default()) else {
            panic!()
        };
        let time = |f: &dyn Fn() -> usize| {
            let t = std::time::Instant::now();
            let n = f();
            (t.elapsed(), n)
        };
        let count = |spec: &FilterSpec| {
            let mut out = FilterOutput::default();
            spec.run(&s, 0..s.len(), &mut out);
            out.filtered.len()
        };
        let pattern = "{d}.{m}.{y}";
        if let Some(ts) = s.timestamp(s.len() / 2) {
            let day = crate::store::ms_to_iso(ts);
            let day = crate::colfilter::DateLayout::new(pattern).format_iso(&day[..10]);
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
            let range = spec(&day);
            let new = time(&|| count(&range));
            let fallback = spec(":30");
            let new_text = time(&|| count(&fallback));
            println!(
                "BENCH date filter {day:?}: text {:.2?} ({}) → range {:.2?} ({}); \
                 \":30\" on stack {:.2?} ({})",
                old.0, old.1, new.0, new.1, new_text.0, new_text.1
            );
        }
        let second = std::env::var("RUSTRACE_BENCH_SEARCH2").unwrap_or_else(|_| "jdoe".into());
        for text in ["error 0x8007", second.as_str()] {
            let search = FilterSpec {
                search: Some(Regex::new(&format!("(?i){}", regex::escape(text))).unwrap()),
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
                "BENCH entries={} extra columns={} search {text:?}: sequential {:.2?} ({}) → \
                 parallel {:.2?} ({})",
                s.len(),
                s.at(0).extras().count(),
                seq.0,
                seq.1,
                par.0,
                par.1
            );
        }
    }
}
