//! Compact storage of log entries.
//!
//! Instead of a `Vec<LogEntry>` with four `String`s per entry (four heap allocations,
//! ~130 bytes of overhead per entry), the entries are kept as:
//!
//! * a 32-byte record per entry (`Rec`): start of the message in the text arena, date/time as
//!   milliseconds (`i64`), line number, interned component and thread, severity,
//! * one text arena with all messages, in entry order (a message ends where the next one starts),
//! * an interner for values that repeat (components, threads, unparsed dates, extra columns),
//! * the extra columns of a log definition column by column as interned ids.
//!
//! The text arena is either in memory or in a **memory-mapped temporary file**
//! (`FILE_ATTRIBUTE_TEMPORARY` + `FILE_FLAG_DELETE_ON_CLOSE`). While memory is free Windows keeps
//! the file in its cache (as fast as memory); when memory is short it can drop the pages instead
//! of swapping the process. The original log is never mapped – that would stop the program that
//! writes it from truncating or rotating it.

use crate::parser::{LogEntry, Severity};
use memmap2::MmapMut;
use std::collections::HashMap;
use std::fs::File;
use std::hash::{BuildHasherDefault, Hasher};

// ----------------------------------------------------------------------
// Date/time as milliseconds
// ----------------------------------------------------------------------

/// No date/time.
const TS_NONE: i64 = i64::MIN;
/// Date/time text that is not ISO: `TS_RAW + interned id`. Real timestamps (years 0–9999) are
/// far away from `i64::MIN`, so the ranges never overlap.
const TS_RAW: i64 = i64::MIN + 1;
const TS_RAW_END: i64 = TS_RAW + (1 << 32);

/// Days since 1970-01-01 of a civil date (proleptic Gregorian calendar).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// "YYYY-MM-DD HH:MM:SS.mmm" or "YYYY-MM-DD HH:MM:SS" → milliseconds since 1970 (no time zone).
pub fn iso_to_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if !(b.len() == 19 || b.len() == 23) {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        b[r].iter().try_fold(0i64, |acc, &c| {
            c.is_ascii_digit().then(|| acc * 10 + (c - b'0') as i64)
        })
    };
    let sep = |i: usize, c: u8| b[i] == c;
    if !(sep(4, b'-') && sep(7, b'-') && sep(10, b' ') && sep(13, b':') && sep(16, b':')) {
        return None;
    }
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let ms = if b.len() == 23 {
        if !sep(19, b'.') {
            return None;
        }
        num(20..23)?
    } else {
        0
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mdays = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&m)
        || d < 1
        || d > mdays[(m - 1) as usize]
        || hh > 23
        || mm > 59
        || ss > 59
    {
        return None;
    }
    Some(((days_from_civil(y, m, d) * 24 + hh) * 60 + mm) * 60_000 + ss * 1000 + ms)
}

/// Milliseconds since 1970 → "YYYY-MM-DD HH:MM:SS.mmm".
pub fn ms_to_iso(ms: i64) -> String {
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let (y, m, d) = civil_from_days(days);
    if !(0..=9999).contains(&y) {
        return format!("{y}-{m:02}-{d:02}");
    }
    // Written digit by digit – called for every visible row, `format!` is several times slower.
    let mut b = *b"0000-00-00 00:00:00.000";
    let mut put = |pos: usize, width: usize, mut v: i64| {
        for i in (pos..pos + width).rev() {
            b[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
    };
    put(0, 4, y);
    put(5, 2, m);
    put(8, 2, d);
    put(11, 2, rem / 3_600_000);
    put(14, 2, rem / 60_000 % 60);
    put(17, 2, rem / 1000 % 60);
    put(20, 3, rem % 1000);
    b.iter().map(|&c| c as char).collect()
}

// ----------------------------------------------------------------------
// Interner
// ----------------------------------------------------------------------

/// Fast hash of a string (FxHash-like, 8 bytes at a time).
fn hash_str(s: &str) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mut h: u64 = s.len() as u64;
    let (chunks, rest) = s.as_bytes().as_chunks::<8>();
    for c in chunks {
        h = (h.rotate_left(5) ^ u64::from_le_bytes(*c)).wrapping_mul(K);
    }
    let mut last = [0u8; 8];
    last[..rest.len()].copy_from_slice(rest);
    (h.rotate_left(5) ^ u64::from_le_bytes(last)).wrapping_mul(K)
}

/// The keys of the map are already hashes.
#[derive(Default)]
struct IdentityHasher(u64);

impl Hasher for IdentityHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, _: &[u8]) {
        unreachable!("only u64 keys")
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = v;
    }
}

const NO_ID: u32 = u32::MAX;

/// Stores every distinct string once; id 0 is the empty string.
struct Interner {
    /// All strings one after another; string `i` is `bytes[starts[i]..starts[i + 1]]`.
    bytes: String,
    starts: Vec<usize>,
    /// Hash → first id with that hash; further ids with the same hash are chained in `next`.
    map: HashMap<u64, u32, BuildHasherDefault<IdentityHasher>>,
    next: Vec<u32>,
}

impl Interner {
    fn new() -> Self {
        let mut i = Self {
            bytes: String::new(),
            starts: vec![0],
            map: HashMap::default(),
            next: Vec::new(),
        };
        i.intern("");
        i
    }

    fn len(&self) -> usize {
        self.next.len()
    }

    fn get(&self, id: u32) -> &str {
        let id = id as usize;
        &self.bytes[self.starts[id]..self.starts[id + 1]]
    }

    fn intern(&mut self, s: &str) -> u32 {
        if s.is_empty() && !self.next.is_empty() {
            return 0;
        }
        let h = hash_str(s);
        let first = self.map.get(&h).copied().unwrap_or(NO_ID);
        let mut id = first;
        while id != NO_ID {
            if self.get(id) == s {
                return id;
            }
            id = self.next[id as usize];
        }
        let new = self.next.len() as u32;
        self.bytes.push_str(s);
        self.starts.push(self.bytes.len());
        self.next.push(first);
        self.map.insert(h, new);
        new
    }
}

// ----------------------------------------------------------------------
// Text arena
// ----------------------------------------------------------------------

/// Text of all messages, in memory or in a memory-mapped temporary file.
enum TextArena {
    Memory(String),
    Mapped(MappedText),
}

/// Minimum size of the temporary file (it grows by doubling).
const MIN_MAPPED: usize = 64 << 20;

struct MappedText {
    file: File,
    map: Option<MmapMut>,
    /// Bytes in use (the file and the mapping are larger).
    len: usize,
}

/// Creates a temporary file that only this process can open and that Windows deletes when it
/// is closed (also when the program crashes). Elsewhere the file is unlinked right away.
fn temp_file() -> std::io::Result<File> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir();
    loop {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("rustrace-{}-{n}.tmp", std::process::id()));
        let mut o = std::fs::OpenOptions::new();
        o.read(true).write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_ATTRIBUTE_TEMPORARY: u32 = 0x100;
            const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
            o.share_mode(0)
                .attributes(FILE_ATTRIBUTE_TEMPORARY)
                .custom_flags(FILE_FLAG_DELETE_ON_CLOSE);
        }
        match o.open(&path) {
            Ok(f) => {
                #[cfg(not(windows))]
                let _ = std::fs::remove_file(&path);
                return Ok(f);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 1000 => continue,
            Err(e) => return Err(e),
        }
    }
}

impl MappedText {
    fn new() -> std::io::Result<Self> {
        Ok(Self {
            file: temp_file()?,
            map: None,
            len: 0,
        })
    }

    fn capacity(&self) -> usize {
        self.map.as_ref().map_or(0, |m| m.len())
    }

    /// Makes room for `need` bytes in total: enlarges the file, then maps it again.
    /// On error the old mapping stays valid.
    fn reserve_total(&mut self, need: usize) -> std::io::Result<()> {
        if need <= self.capacity() {
            return Ok(());
        }
        let cap = need
            .max(self.capacity() * 2)
            .max(MIN_MAPPED)
            .next_multiple_of(1 << 20);
        self.file.set_len(cap as u64)?;
        // SAFETY: the file is opened exclusively (share mode 0, deleted on close), so no other
        // process can change it; only this struct writes it and only within `len`/`cap`.
        let map = unsafe { MmapMut::map_mut(&self.file)? };
        self.map = Some(map);
        Ok(())
    }

    fn push_str(&mut self, s: &str) -> std::io::Result<()> {
        self.reserve_total(self.len + s.len())?;
        if let Some(m) = self.map.as_mut() {
            m[self.len..self.len + s.len()].copy_from_slice(s.as_bytes());
        }
        self.len += s.len();
        Ok(())
    }

    fn as_str(&self) -> &str {
        match &self.map {
            // SAFETY: only whole `&str`s are written and `truncate` cuts at entry boundaries,
            // so `[..len]` is always valid UTF-8.
            Some(m) => unsafe { std::str::from_utf8_unchecked(&m[..self.len]) },
            None => "",
        }
    }
}

impl TextArena {
    fn len(&self) -> usize {
        match self {
            TextArena::Memory(t) => t.len(),
            TextArena::Mapped(m) => m.len,
        }
    }

    fn as_str(&self) -> &str {
        match self {
            TextArena::Memory(t) => t,
            TextArena::Mapped(m) => m.as_str(),
        }
    }

    /// If the temporary file cannot grow (disk full …), the text moves to memory.
    fn move_to_memory(&mut self, extra: usize) {
        let mut t = String::with_capacity(self.len() + extra);
        t.push_str(self.as_str());
        *self = TextArena::Memory(t);
    }

    fn push_str(&mut self, s: &str) {
        match self {
            TextArena::Memory(t) => t.push_str(s),
            TextArena::Mapped(m) => {
                if m.push_str(s).is_err() {
                    self.move_to_memory(s.len());
                    self.push_str(s);
                }
            }
        }
    }

    fn reserve(&mut self, additional: usize) {
        match self {
            TextArena::Memory(t) => t.reserve(additional),
            TextArena::Mapped(m) => {
                if m.reserve_total(m.len + additional).is_err() {
                    self.move_to_memory(additional);
                }
            }
        }
    }

    fn truncate(&mut self, len: usize) {
        match self {
            TextArena::Memory(t) => t.truncate(len),
            TextArena::Mapped(m) => m.len = m.len.min(len),
        }
    }

    /// Removes the first `n` bytes (an entry boundary); the rest moves to the front.
    fn drop_front(&mut self, n: usize) {
        match self {
            TextArena::Memory(t) => {
                t.drain(..n);
            }
            TextArena::Mapped(m) => {
                let len = m.len;
                if let Some(map) = m.map.as_mut() {
                    map.copy_within(n..len, 0);
                }
                m.len = len - n;
            }
        }
    }
}

// ----------------------------------------------------------------------
// The store
// ----------------------------------------------------------------------

/// One entry – 32 bytes.
#[derive(Clone, Copy)]
struct Rec {
    /// Start of the message in the text arena (it ends where the next message starts).
    start: u64,
    /// Milliseconds since 1970, `TS_NONE`, or `TS_RAW + id` for a date that is not ISO.
    ts: i64,
    line: u32,
    component: u32,
    thread: u32,
    /// Which file of the log (rotated parts + active file) the entry comes from.
    file: u16,
    severity: Severity,
}

/// All entries of a log.
pub struct LogStore {
    recs: Vec<Rec>,
    text: TextArena,
    strings: Interner,
    /// Extra columns of a log definition: `extra[column][entry]` = interned id.
    extra: Vec<Vec<u32>>,
}

impl Default for LogStore {
    fn default() -> Self {
        Self::new()
    }
}

/// A view of one entry (all texts borrowed from the store).
#[derive(Clone, Copy)]
pub struct EntryRef<'a> {
    store: &'a LogStore,
    idx: usize,
    pub line_no: usize,
    pub message: &'a str,
    pub component: &'a str,
    pub thread: &'a str,
    pub severity: Severity,
    /// File id (index into `LogSource::files`).
    pub file: u16,
}

impl<'a> EntryRef<'a> {
    /// Date/time as ISO text ("YYYY-MM-DD HH:MM:SS.mmm"), the original text if it is not
    /// a date, or "".
    pub fn datetime(&self) -> String {
        self.store.datetime(self.idx)
    }

    pub fn timestamp(&self) -> Option<i64> {
        self.store.timestamp(self.idx)
    }

    /// Value of an extra column ("" when missing).
    pub fn extra(&self, col: usize) -> &'a str {
        self.store.extra(self.idx, col)
    }

    /// Values of all extra columns.
    pub fn extras(&self) -> impl Iterator<Item = &'a str> + 'a {
        let (store, idx) = (self.store, self.idx);
        store.extra.iter().map(move |c| store.strings.get(c[idx]))
    }
}

impl LogStore {
    /// Store with the text in memory.
    pub fn new() -> Self {
        Self {
            recs: Vec::new(),
            text: TextArena::Memory(String::new()),
            strings: Interner::new(),
            extra: Vec::new(),
        }
    }

    /// Store with the text in a memory-mapped temporary file (in memory if it cannot be created).
    pub fn new_mapped() -> Self {
        let mut s = Self::new();
        if let Ok(m) = MappedText::new() {
            s.text = TextArena::Mapped(m);
        }
        s
    }

    /// The text is in a temporary file.
    pub fn is_mapped(&self) -> bool {
        matches!(self.text, TextArena::Mapped(_))
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// Bytes of message text.
    pub fn text_len(&self) -> usize {
        self.text.len()
    }

    /// Makes room for more entries and message text (avoids repeated growing).
    pub fn reserve(&mut self, entries: usize, text_bytes: usize) {
        self.recs.reserve(entries);
        self.text.reserve(text_bytes);
    }

    /// Adds an entry of file `file` (index into `LogSource::files`).
    pub fn push(&mut self, e: LogEntry, file: u16) {
        let start = self.text.len() as u64;
        self.text.push_str(&e.message);
        let ts = if e.datetime.is_empty() {
            TS_NONE
        } else {
            iso_to_ms(&e.datetime)
                .unwrap_or_else(|| TS_RAW + self.strings.intern(&e.datetime) as i64)
        };
        self.recs.push(Rec {
            start,
            ts,
            line: e.line_no.min(u32::MAX as usize) as u32,
            component: self.strings.intern(&e.component),
            thread: self.strings.intern(&e.thread),
            file,
            severity: e.severity,
        });
        let n = self.recs.len();
        while self.extra.len() < e.extra.len() {
            self.extra.push(vec![0; n - 1]);
        }
        for (c, col) in self.extra.iter_mut().enumerate() {
            col.push(e.extra.get(c).map_or(0, |v| self.strings.intern(v)));
        }
    }

    /// Moves the text from memory to a memory-mapped temporary file (when a watched log grew
    /// big). Returns false when the temporary file cannot be used (the text stays in memory).
    pub fn move_text_to_temp_file(&mut self) -> bool {
        let TextArena::Memory(t) = &self.text else {
            return true;
        };
        let Ok(mut m) = MappedText::new() else {
            return false;
        };
        if m.reserve_total(t.len())
            .and_then(|_| m.push_str(t))
            .is_err()
        {
            return false;
        }
        self.text = TextArena::Mapped(m);
        true
    }

    /// Index of the first entry whose message starts at or after text position `pos`.
    pub fn first_at_or_after(&self, pos: usize) -> usize {
        self.recs.partition_point(|r| (r.start as usize) < pos)
    }

    /// Removes the oldest `k` entries (memory limit of a long-watched log). The interner is
    /// rebuilt, so values used only by the removed entries are freed too.
    pub fn drop_front(&mut self, k: usize) {
        let k = k.min(self.recs.len());
        if k == 0 {
            return;
        }
        let base = self
            .recs
            .get(k)
            .map_or(self.text.len(), |r| r.start as usize);
        self.text.drop_front(base);
        self.recs.drain(..k);
        for c in &mut self.extra {
            c.drain(..k);
        }
        let old = std::mem::replace(&mut self.strings, Interner::new());
        let mut ids = vec![NO_ID; old.len()];
        let strings = &mut self.strings;
        let mut map = |id: u32| {
            let slot = &mut ids[id as usize];
            if *slot == NO_ID {
                *slot = strings.intern(old.get(id));
            }
            *slot
        };
        for r in &mut self.recs {
            r.start -= base as u64;
            r.component = map(r.component);
            r.thread = map(r.thread);
            if (TS_RAW..TS_RAW_END).contains(&r.ts) {
                r.ts = TS_RAW + map((r.ts - TS_RAW) as u32) as i64;
            }
        }
        for c in &mut self.extra {
            for id in c.iter_mut() {
                *id = map(*id);
            }
        }
    }

    /// Keeps only the first `n` entries.
    pub fn truncate(&mut self, n: usize) {
        if n >= self.recs.len() {
            return;
        }
        self.text.truncate(self.recs[n].start as usize);
        self.recs.truncate(n);
        for c in &mut self.extra {
            c.truncate(n);
        }
    }

    /// Moves the entries of `other` to the end of this store.
    pub fn append(&mut self, other: LogStore) {
        let base = self.text.len() as u64;
        let n = self.recs.len();
        self.text.push_str(other.text.as_str());
        let mut ids = vec![NO_ID; other.strings.len()];
        let strings = &mut self.strings;
        let mut map = |id: u32| {
            let slot = &mut ids[id as usize];
            if *slot == NO_ID {
                *slot = strings.intern(other.strings.get(id));
            }
            *slot
        };
        self.recs.reserve(other.recs.len());
        for r in &other.recs {
            let ts = if (TS_RAW..TS_RAW_END).contains(&r.ts) {
                TS_RAW + map((r.ts - TS_RAW) as u32) as i64
            } else {
                r.ts
            };
            self.recs.push(Rec {
                start: base + r.start,
                ts,
                component: map(r.component),
                thread: map(r.thread),
                ..*r
            });
        }
        while self.extra.len() < other.extra.len() {
            self.extra.push(vec![0; n]);
        }
        for (c, col) in self.extra.iter_mut().enumerate() {
            match other.extra.get(c) {
                Some(src) => col.extend(src.iter().map(|&id| map(id))),
                None => col.resize(n + other.recs.len(), 0),
            }
        }
    }

    pub fn get(&self, i: usize) -> Option<EntryRef<'_>> {
        (i < self.recs.len()).then(|| self.at(i))
    }

    /// Entry `i` (panics when out of range).
    pub fn at(&self, i: usize) -> EntryRef<'_> {
        let r = &self.recs[i];
        EntryRef {
            store: self,
            idx: i,
            line_no: r.line as usize,
            message: self.message(i),
            component: self.strings.get(r.component),
            thread: self.strings.get(r.thread),
            severity: r.severity,
            file: r.file,
        }
    }

    pub fn message(&self, i: usize) -> &str {
        let start = self.recs[i].start as usize;
        let end = self
            .recs
            .get(i + 1)
            .map_or(self.text.len(), |r| r.start as usize);
        &self.text.as_str()[start..end]
    }

    #[cfg(test)]
    pub fn severity(&self, i: usize) -> Severity {
        self.recs[i].severity
    }

    pub fn line_no(&self, i: usize) -> usize {
        self.recs[i].line as usize
    }

    /// File id of entry `i`.
    pub fn file(&self, i: usize) -> u16 {
        self.recs[i].file
    }

    /// Milliseconds since 1970 (`None` without an ISO date/time).
    pub fn timestamp(&self, i: usize) -> Option<i64> {
        let ts = self.recs[i].ts;
        (ts >= TS_RAW_END).then_some(ts)
    }

    pub fn datetime(&self, i: usize) -> String {
        match self.recs[i].ts {
            TS_NONE => String::new(),
            ts if ts < TS_RAW_END => self.strings.get((ts - TS_RAW) as u32).to_owned(),
            ts => ms_to_iso(ts),
        }
    }

    pub fn extra(&self, i: usize, col: usize) -> &str {
        self.extra.get(col).map_or("", |c| self.strings.get(c[i]))
    }

    /// Builds a store from entries (tests).
    #[cfg(test)]
    pub fn from_entries(entries: impl IntoIterator<Item = LogEntry>) -> Self {
        let mut s = Self::new();
        for e in entries {
            s.push(e, 0);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(line_no: usize, msg: &str, comp: &str, dt: &str, extra: &[&str]) -> LogEntry {
        LogEntry {
            line_no,
            message: msg.to_owned(),
            component: comp.to_owned(),
            datetime: dt.to_owned(),
            thread: "7 (0x0007)".to_owned(),
            severity: Severity::Warning,
            extra: extra.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn record_is_32_bytes() {
        assert_eq!(std::mem::size_of::<Rec>(), 32);
    }

    #[test]
    fn iso_round_trip() {
        for s in [
            "2026-10-23 13:22:01.123",
            "1970-01-01 00:00:00.000",
            "1601-02-28 23:59:59.999",
            "2024-02-29 12:00:00.000",
        ] {
            assert_eq!(ms_to_iso(iso_to_ms(s).unwrap()), s);
        }
        assert_eq!(iso_to_ms("1970-01-01 00:00:01").unwrap(), 1000);
        assert!(iso_to_ms("2026-02-29 00:00:00").is_none());
        assert!(iso_to_ms("2026-10-23T13:22:01.123").is_none());
        assert!(iso_to_ms("13:22:01.123").is_none());
        assert!(iso_to_ms("2026-10-23 24:00:00").is_none());
        // Ordering of milliseconds equals ordering of the ISO text.
        assert!(
            iso_to_ms("2026-10-23 10:00:00.001").unwrap()
                > iso_to_ms("2026-10-23 09:59:59.999").unwrap()
        );
    }

    #[test]
    fn push_get_truncate_append() {
        let mut s = LogStore::new();
        s.push(entry(1, "one", "CompA", "2026-10-23 10:00:00.000", &[]), 0);
        s.push(entry(2, "two\nlines", "CompB", "13:00:00", &["x", "y"]), 0);
        s.push(entry(4, "", "CompA", "", &["z"]), 0);
        assert_eq!(s.len(), 3);
        let e = s.at(1);
        assert_eq!(
            (e.line_no, e.message, e.component, e.thread),
            (2, "two\nlines", "CompB", "7 (0x0007)")
        );
        assert_eq!(e.severity, Severity::Warning);
        assert_eq!(s.datetime(0), "2026-10-23 10:00:00.000");
        assert_eq!(s.timestamp(0), iso_to_ms("2026-10-23 10:00:00.000"));
        assert_eq!(s.datetime(1), "13:00:00"); // not ISO → kept as text
        assert_eq!(s.timestamp(1), None);
        assert_eq!(s.datetime(2), "");
        assert_eq!(s.message(2), "");
        // Extra columns: the first entry had none, the third only the first one.
        assert_eq!(
            (s.extra(0, 0), s.extra(1, 1), s.extra(2, 0), s.extra(2, 1)),
            ("", "y", "z", "")
        );
        assert_eq!(s.at(1).extras().collect::<Vec<_>>(), ["x", "y"]);
        // Components are stored once.
        assert_eq!(s.strings.len(), 1 + 3 + 1 + 3); // "", CompA, thread, CompB, "13:00:00", x, y, z

        s.truncate(1);
        assert_eq!(s.len(), 1);
        assert_eq!(s.text_len(), 3);
        s.push(entry(5, "five", "CompC", "2026-10-23 11:00:00.000", &[]), 0);
        assert_eq!(s.message(0), "one");
        assert_eq!(s.message(1), "five");

        let mut other = LogStore::new();
        other.push(entry(6, "six", "CompB", "raw date", &["", "q"]), 0);
        s.append(other);
        assert_eq!(s.len(), 3);
        let e = s.at(2);
        assert_eq!((e.message, e.component, e.line_no), ("six", "CompB", 6));
        assert_eq!(s.datetime(2), "raw date");
        assert_eq!((s.extra(2, 1), s.extra(0, 1)), ("q", ""));
    }

    #[test]
    fn mapped_text_grows_and_truncates() {
        let mut s = LogStore::new_mapped();
        assert!(s.is_mapped());
        let line = "x".repeat(1000) + "ž";
        // More than the first 64 MB of the temporary file → it has to grow and be mapped again.
        let n = 70_000;
        for i in 0..n {
            s.push(entry(i + 1, &line, "C", "", &[]), 0);
        }
        assert!(s.text_len() > MIN_MAPPED);
        assert_eq!(s.message(n - 1), line);
        assert_eq!(s.message(12_345), line);
        s.truncate(10);
        assert_eq!(s.text_len(), 10 * line.len());
        s.push(entry(11, "end", "C", "", &[]), 0);
        assert_eq!(s.message(10), "end");
    }

    /// Removing the oldest entries (memory limit), in memory and in the temporary file.
    #[test]
    fn drop_front_and_move_to_temp_file() {
        for mapped in [false, true] {
            let mut s = LogStore::new();
            s.push(entry(1, "old one", "OldComp", "raw old", &["gone"]), 0);
            s.push(entry(2, "old two", "Comp", "", &["x"]), 0);
            s.push(
                entry(3, "keep ž", "Comp", "2026-10-23 10:00:00.000", &["y"]),
                1,
            );
            s.push(entry(4, "last", "Comp2", "raw new", &["y"]), 1);
            if mapped {
                assert!(s.move_text_to_temp_file());
                assert!(s.is_mapped());
            }
            assert_eq!(s.first_at_or_after(0), 0);
            assert_eq!(s.first_at_or_after(7), 1); // "old one" is 7 bytes
            assert_eq!(s.first_at_or_after(8), 2);
            s.drop_front(2);
            assert_eq!(s.len(), 2);
            let e = s.at(0);
            assert_eq!(
                (e.message, e.component, e.line_no, e.file),
                ("keep ž", "Comp", 3, 1)
            );
            assert_eq!(s.datetime(0), "2026-10-23 10:00:00.000");
            assert_eq!(s.datetime(1), "raw new");
            assert_eq!((s.extra(0, 0), s.extra(1, 0)), ("y", "y"));
            assert_eq!(s.text_len(), "keep ž".len() + "last".len());
            // Values of the removed entries are gone from the interner:
            // "", thread, Comp, "y", Comp2, "raw new".
            assert_eq!(s.strings.len(), 6);
            // Appending keeps working after the removal.
            s.push(entry(5, "new", "Comp", "", &[]), 1);
            assert_eq!(s.message(1), "last");
            assert_eq!(s.message(2), "new");
            s.drop_front(10);
            assert!(s.is_empty());
            assert_eq!(s.text_len(), 0);
        }
    }

    #[test]
    fn interner_collisions_and_empty() {
        let mut i = Interner::new();
        assert_eq!(i.intern(""), 0);
        let a = i.intern("abc");
        assert_eq!(i.intern("abc"), a);
        let ids: Vec<u32> = (0..10_000)
            .map(|n| i.intern(&format!("value{n}")))
            .collect();
        for (n, id) in ids.iter().enumerate() {
            assert_eq!(i.get(*id), format!("value{n}"));
        }
        assert_eq!(i.len(), 2 + 10_000);
    }
}
