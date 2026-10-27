//! Several different logs in one view, interleaved by date/time (#21).
//!
//! A `LogSet` reads one or more logs (`LogSource`s, each with its rotated parts). With one log
//! its results pass through unchanged. With several logs the entries are merged into one store:
//! * the merge key of an entry is its timestamp; an entry without one gets the key of the entry
//!   before it in the same log, so it stays right after it (the first ones go to the start),
//! * the order inside every log is kept; equal keys keep the order of the logs,
//! * new entries usually go at the end; when one is older than the newest entry of the view,
//!   the end of the view after that time is merged again with them (`apply_appended`),
//! * the files get ids of the merged view (`files`) and the columns are united: the standard
//!   columns once, extra columns of log definitions by name (`schema`).
//!
//! When a log that was already read is truncated or replaced, all logs are read again.

use crate::logdef::{Column, ColumnKind, LogDef, Schema};
use crate::parser::{FormatChoice, LogEntry, LogSource, PollResult, Progress, SourceFile};
use crate::store::LogStore;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::PathBuf;
use std::sync::Arc;

/// Merge key of entries before the first one with a date/time.
const KEY_MIN: i64 = i64::MIN;

/// Result of reading all logs of a set once.
pub enum ReadResult {
    Unchanged,
    /// Everything was read from the start.
    Reset {
        committed: LogStore,
        provisional: LogStore,
    },
    /// New entries. `from_key` (merged view only): some are older than the newest entry of the
    /// view, the entries after that time have to be merged again with them (`apply_appended`).
    Appended {
        committed: LogStore,
        provisional: LogStore,
        from_key: Option<i64>,
    },
}

/// Interleaves sequences by their merge keys: always the sequence whose next entry has the
/// smallest key (equal keys: the lower sequence first); the order inside a sequence is kept.
/// Returns (sequence, index in the sequence) in the merged order.
pub fn interleave(keys: &[Vec<i64>]) -> Vec<(usize, usize)> {
    let mut heap: BinaryHeap<Reverse<(i64, usize)>> = keys
        .iter()
        .enumerate()
        .filter_map(|(s, k)| k.first().map(|&k| Reverse((k, s))))
        .collect();
    let mut pos = vec![0; keys.len()];
    let mut out = Vec::with_capacity(keys.iter().map(Vec::len).sum());
    while let Some(Reverse((_, s))) = heap.pop() {
        out.push((s, pos[s]));
        pos[s] += 1;
        if let Some(&k) = keys[s].get(pos[s]) {
            heap.push(Reverse((k, s)));
        }
    }
    out
}

/// Merge key of an entry: its timestamp, else the key before it (`carry`).
fn key(ts: Option<i64>, carry: &mut i64) -> i64 {
    if let Some(t) = ts {
        *carry = t;
    }
    *carry
}

/// Adds the new entries of a refresh to the view (`committed_len` final entries followed by
/// provisional ones). Returns the new number of final entries and the index of the first entry
/// that changed (entries from there on have to be filtered again).
pub fn apply_appended(
    view: &mut LogStore,
    committed_len: usize,
    committed: LogStore,
    provisional: LogStore,
    from_key: Option<i64>,
    file_sources: &[usize],
) -> (usize, usize) {
    view.truncate(committed_len);
    let changed = match from_key {
        Some(k) => merge_tail(view, committed, k, file_sources),
        None => {
            view.append(committed);
            committed_len
        }
    };
    let len = view.len();
    view.append(provisional);
    (len, changed)
}

/// Merges `new` into the view: the entries after the last one dated at or before `from_key` are
/// interleaved again with them, by log (`file_sources`: file id → log). Returns where the
/// changed part starts.
fn merge_tail(view: &mut LogStore, new: LogStore, from_key: i64, file_sources: &[usize]) -> usize {
    let mut p = view.len();
    while p > 0 && view.timestamp(p - 1).is_none_or(|t| t > from_key) {
        p -= 1;
    }
    let floor = p
        .checked_sub(1)
        .and_then(|i| view.timestamp(i))
        .unwrap_or(KEY_MIN);
    let tail = view.split_off(p);
    let logs = file_sources.iter().max().map_or(1, |m| m + 1);
    let log_of = |f: u16| file_sources.get(f as usize).copied().unwrap_or(0);
    // Per log: its entries of the old tail, then its new ones.
    let mut items: Vec<Vec<(bool, usize)>> = vec![Vec::new(); logs];
    let mut keys: Vec<Vec<i64>> = vec![Vec::new(); logs];
    let mut carry = vec![floor; logs];
    for (is_new, store) in [(false, &tail), (true, &new)] {
        for i in 0..store.len() {
            let s = log_of(store.file(i));
            keys[s].push(key(store.timestamp(i), &mut carry[s]));
            items[s].push((is_new, i));
        }
    }
    let (mut tail_ids, mut new_ids) = (Vec::new(), Vec::new());
    for (s, k) in interleave(&keys) {
        let (is_new, i) = items[s][k];
        let (store, ids) = if is_new {
            (&new, &mut new_ids)
        } else {
            (&tail, &mut tail_ids)
        };
        view.copy_entry(store, i, store.file(i), ids, None);
    }
    p
}

/// One or more logs read as one view.
pub struct LogSet {
    sources: Vec<LogSource>,
    /// Settings handed to every log before it is read.
    pub keyword_severity: bool,
    pub defs: Arc<Vec<Arc<LogDef>>>,
    pub format: FormatChoice,
    pub temp_file_threshold: Option<u64>,
    pub merge_rotated: bool,
    /// Files of the view (file id = index), the log each one belongs to, and the file id of
    /// every file of every log (`map[log][file id in the log]`).
    files: Vec<SourceFile>,
    file_sources: Vec<usize>,
    map: Vec<Vec<u16>>,
    /// Columns of the view; extra column of the view for every extra column of every log.
    schema: Schema,
    extra_map: Vec<Vec<usize>>,
    /// Merge key of the last final entry of every log, and the newest key in the view.
    last_key: Vec<i64>,
    newest: i64,
    /// Provisional entries of every log from its last read.
    provisional: Vec<Vec<LogEntry>>,
    /// The log has been read (its next full read means it was truncated or replaced).
    loaded: Vec<bool>,
    /// Logs that could not be read in the last round (merged view; one log alone returns
    /// its error).
    errors: Vec<String>,
}

impl LogSet {
    pub fn new(paths: &[PathBuf], keyword_severity: bool) -> Self {
        let n = paths.len();
        Self {
            sources: paths
                .iter()
                .map(|p| LogSource::new(p, keyword_severity))
                .collect(),
            keyword_severity,
            defs: Arc::new(Vec::new()),
            format: FormatChoice::Auto,
            temp_file_threshold: None,
            merge_rotated: false,
            files: Vec::new(),
            file_sources: Vec::new(),
            map: vec![Vec::new(); n],
            schema: Schema::default(),
            extra_map: vec![Vec::new(); n],
            last_key: vec![KEY_MIN; n],
            newest: KEY_MIN,
            provisional: vec![Vec::new(); n],
            loaded: vec![false; n],
            errors: Vec::new(),
        }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.sources.iter().map(|s| s.path.clone()).collect()
    }

    fn merged(&self) -> bool {
        self.sources.len() > 1
    }

    /// Files of the view (index = file id of the entries).
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// The log (index into the paths) of every file.
    pub fn file_sources(&self) -> &[usize] {
        &self.file_sources
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The log definition of every log (`None` = built-in parser).
    pub fn source_defs(&self) -> Vec<Option<Arc<LogDef>>> {
        self.sources.iter().map(|s| s.active_def()).collect()
    }

    /// The definition all logs are read with (`None` = built-in parser or different ones).
    pub fn active_def(&self) -> Option<Arc<LogDef>> {
        let defs = self.source_defs();
        let first = defs.first().cloned().flatten();
        defs.iter()
            .all(|d| {
                d.as_ref().map(|d| (&d.id, d.priority))
                    == first.as_ref().map(|d| (&d.id, d.priority))
            })
            .then_some(first)
            .flatten()
    }

    /// The logs are read with different formats.
    pub fn mixed_formats(&self) -> bool {
        let defs = self.source_defs();
        let id = |d: &Option<Arc<LogDef>>| d.as_ref().map(|d| (d.id.clone(), d.priority));
        defs.iter().any(|d| id(d) != id(&defs[0]))
    }

    /// Encoding of the log ("" for a merged view – see `SourceFile::encoding`).
    pub fn encoding_name(&self) -> &'static str {
        match self.sources.as_slice() {
            [one] => one.encoding_name(),
            _ => "",
        }
    }

    pub fn file_size(&self) -> u64 {
        self.sources.iter().map(|s| s.file_size).sum()
    }

    /// Logs that could not be read in the last round of a merged view.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Reads new data of all logs (in the background thread).
    pub fn poll(&mut self, progress: &Progress) -> std::io::Result<ReadResult> {
        for s in &mut self.sources {
            s.keyword_severity = self.keyword_severity;
            s.defs = self.defs.clone();
            s.format = self.format.clone();
            s.temp_file_threshold = self.temp_file_threshold;
            s.merge_rotated = self.merge_rotated;
        }
        if !self.merged() {
            return self.poll_single(progress);
        }
        self.errors.clear();
        let mut results = Vec::new();
        for s in &mut self.sources {
            match s.poll(progress) {
                Ok(r) => results.push(r),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Err(e),
                Err(e) => {
                    self.errors.push(format!("{}: {e}", s.path.display()));
                    results.push(PollResult::Unchanged);
                }
            }
        }
        // A log read before was truncated or replaced: the view is built again.
        let reread = results
            .iter()
            .zip(&self.loaded)
            .any(|(r, &loaded)| loaded && matches!(r, PollResult::Reset { .. }));
        if reread {
            let paths = self.paths();
            let errors = std::mem::take(&mut self.errors);
            *self = Self {
                keyword_severity: self.keyword_severity,
                defs: self.defs.clone(),
                format: self.format.clone(),
                temp_file_threshold: self.temp_file_threshold,
                merge_rotated: self.merge_rotated,
                ..Self::new(&paths, self.keyword_severity)
            };
            let res = self.poll(progress);
            self.errors.extend(errors);
            return res;
        }
        self.sync();
        let first = !self.loaded.iter().any(|&l| l);
        let mut batches = Vec::new();
        for (s, r) in results.into_iter().enumerate() {
            match r {
                PollResult::Unchanged => {}
                PollResult::Reset {
                    committed,
                    provisional,
                }
                | PollResult::Appended {
                    committed,
                    provisional,
                } => {
                    self.loaded[s] = true;
                    self.provisional[s] = provisional;
                    batches.push((s, committed));
                }
            }
        }
        if batches.is_empty() && !first {
            return Ok(ReadResult::Unchanged);
        }
        let newest = self.newest;
        let (committed, min_key) = self.merge_batches(&batches, first);
        let provisional = self.provisional_store();
        Ok(if first {
            ReadResult::Reset {
                committed,
                provisional,
            }
        } else {
            ReadResult::Appended {
                committed,
                provisional,
                from_key: min_key.filter(|&k| k < newest),
            }
        })
    }

    /// One log: its results with the provisional entries as a store.
    fn poll_single(&mut self, progress: &Progress) -> std::io::Result<ReadResult> {
        let res = self.sources[0].poll(progress)?;
        let src = &self.sources[0];
        self.files = src.files().to_vec();
        self.file_sources = vec![0; self.files.len()];
        self.schema = src.schema().clone();
        let active = self.files.len().saturating_sub(1) as u16;
        let store = |entries: Vec<LogEntry>| {
            let mut s = LogStore::new();
            for e in entries {
                s.push(e, active);
            }
            s
        };
        Ok(match res {
            PollResult::Unchanged => ReadResult::Unchanged,
            PollResult::Reset {
                committed,
                provisional,
            } => ReadResult::Reset {
                committed,
                provisional: store(provisional),
            },
            PollResult::Appended {
                committed,
                provisional,
            } => ReadResult::Appended {
                committed,
                provisional: store(provisional),
                from_key: None,
            },
        })
    }

    /// New files and extra columns of the logs get their ids in the view; the columns of the
    /// view are united again.
    fn sync(&mut self) {
        for (s, src) in self.sources.iter().enumerate() {
            for (local, f) in src.files().iter().enumerate() {
                match self.map[s].get(local) {
                    Some(&id) => self.files[id as usize] = f.clone(),
                    None => {
                        self.map[s].push(self.files.len() as u16);
                        self.files.push(f.clone());
                        self.file_sources.push(s);
                    }
                }
            }
            let names = &src.schema().extra_names;
            for name in &names[self.extra_map[s].len().min(names.len())..] {
                let all = &mut self.schema.extra_names;
                let col = match all.iter().position(|n| n.eq_ignore_ascii_case(name)) {
                    Some(c) => c,
                    None => {
                        all.push(name.clone());
                        all.len() - 1
                    }
                };
                self.extra_map[s].push(col);
            }
        }
        // Columns of the first log, then those of the others it does not have.
        let mut columns: Vec<Column> = Vec::new();
        for (s, src) in self.sources.iter().enumerate() {
            for c in &src.schema().columns {
                let kind = match c.kind {
                    ColumnKind::Extra(j) => ColumnKind::Extra(self.extra_map[s][j]),
                    k => k,
                };
                if !columns.iter().any(|x| x.kind == kind) {
                    columns.push(Column {
                        kind,
                        title: c.title.clone(),
                    });
                }
            }
        }
        self.schema.columns = columns;
        let mut names: Vec<&str> = Vec::new();
        for src in &self.sources {
            let n = src.schema().name.as_str();
            if !names.contains(&n) {
                names.push(n);
            }
        }
        self.schema.name = if names.len() == 1 {
            names[0].to_owned()
        } else {
            names.join(" + ")
        };
    }

    /// Merges the new final entries of the logs into one store; returns it and its smallest
    /// merge key.
    fn merge_batches(
        &mut self,
        batches: &[(usize, LogStore)],
        first: bool,
    ) -> (LogStore, Option<i64>) {
        let text: u64 = batches.iter().map(|(_, b)| b.text_len() as u64).sum();
        let mut out = if first && self.temp_file_threshold.is_some_and(|t| text >= t) {
            LogStore::new_mapped()
        } else {
            LogStore::new()
        };
        let keys: Vec<Vec<i64>> = batches
            .iter()
            .map(|(s, b)| {
                let carry = &mut self.last_key[*s];
                (0..b.len()).map(|i| key(b.timestamp(i), carry)).collect()
            })
            .collect();
        let min_key = keys.iter().flatten().min().copied();
        if let Some(&max) = keys.iter().flatten().max() {
            self.newest = self.newest.max(max);
        }
        let entries: usize = batches.iter().map(|(_, b)| b.len()).sum();
        out.reserve(entries, text as usize);
        let mut ids = vec![Vec::new(); batches.len()];
        for (b, i) in interleave(&keys) {
            let (s, store) = &batches[b];
            let file = self.map[*s][store.file(i) as usize];
            out.copy_entry(store, i, file, &mut ids[b], Some(&self.extra_map[*s]));
        }
        (out, min_key)
    }

    /// Provisional entries of all logs (unfinished last lines), merged by time.
    fn provisional_store(&self) -> LogStore {
        let mut entries = Vec::new();
        let mut keys = Vec::new();
        for (s, list) in self.provisional.iter().enumerate() {
            let Some(&active) = self.map[s].last() else {
                continue;
            };
            let mut carry = self.last_key[s];
            let mut k = Vec::new();
            let mut e = Vec::new();
            for entry in list {
                let ts = crate::store::iso_to_ms(&entry.datetime);
                k.push(key(ts, &mut carry));
                // Extra columns in the order of the view.
                let mut extra = vec![String::new(); self.schema.extra_names.len()];
                for (j, v) in entry.extra.iter().enumerate() {
                    if let Some(&c) = self.extra_map[s].get(j) {
                        extra[c] = v.clone();
                    }
                }
                e.push((
                    LogEntry {
                        extra,
                        ..entry.clone()
                    },
                    active,
                ));
            }
            keys.push(k);
            entries.push(e);
        }
        let mut out = LogStore::new();
        for (s, i) in interleave(&keys) {
            let (e, file) = &entries[s][i];
            out.push(e.clone(), *file);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;

    fn test_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rustrace_merge_{name}_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cm(msg: &str, time: &str) -> String {
        format!(
            "<![LOG[{msg}]LOG]!><time=\"{time}.000+000\" date=\"10-23-2026\" component=\"C\" context=\"\" type=\"1\" thread=\"1\" file=\"\">\n"
        )
    }

    /// The view as the UI keeps it.
    struct View {
        store: LogStore,
        committed: usize,
    }

    impl View {
        fn apply(&mut self, set: &LogSet, r: ReadResult) {
            match r {
                ReadResult::Unchanged => {}
                ReadResult::Reset {
                    committed,
                    provisional,
                } => {
                    self.store = committed;
                    self.committed = self.store.len();
                    self.store.append(provisional);
                }
                ReadResult::Appended {
                    committed,
                    provisional,
                    from_key,
                } => {
                    let (len, _) = apply_appended(
                        &mut self.store,
                        self.committed,
                        committed,
                        provisional,
                        from_key,
                        set.file_sources(),
                    );
                    self.committed = len;
                }
            }
        }

        fn rows(&self, set: &LogSet) -> Vec<String> {
            (0..self.store.len())
                .map(|i| {
                    let e = self.store.at(i);
                    format!("{}:{}", set.files()[e.file as usize].name, e.message)
                })
                .collect()
        }
    }

    fn read(set: &mut LogSet, view: &mut View) {
        let r = set.poll(&Progress::default()).unwrap();
        view.apply(set, r);
    }

    fn append(p: &Path, text: &str) {
        let mut f = std::fs::OpenOptions::new().append(true).open(p).unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn interleave_keeps_order_inside_sequences() {
        let order = interleave(&[vec![1, 5, 3, 9], vec![2, 5, 4], vec![]]);
        assert_eq!(
            order,
            [(0, 0), (1, 0), (0, 1), (0, 2), (1, 1), (1, 2), (0, 3)]
        );
    }

    /// Two logs ordered by time: equal times keep the order of the logs, lines without a
    /// date/time stay after the line before them, appended lines go to the right place – also
    /// older ones – and the result is the same as reading everything at once.
    #[test]
    fn merged_by_time_and_appended() {
        let dir = test_dir("order");
        let a = dir.join("AppDiscovery.log");
        let b = dir.join("AppEnforce.log");
        std::fs::write(
            &a,
            cm("a1", "10:00:00") + &cm("a2", "10:00:02") + "a2 plain\n" + &cm("a3", "10:00:04"),
        )
        .unwrap();
        std::fs::write(
            &b,
            "b0 plain\n".to_owned() + &cm("b1", "10:00:01") + &cm("b2", "10:00:02"),
        )
        .unwrap();
        let mut set = LogSet::new(&[a.clone(), b.clone()], false);
        let mut view = View {
            store: LogStore::new(),
            committed: 0,
        };
        read(&mut set, &mut view);
        let want = [
            "AppEnforce.log:b0 plain",
            "AppDiscovery.log:a1",
            "AppEnforce.log:b1",
            "AppDiscovery.log:a2",
            "AppDiscovery.log:a2 plain",
            "AppEnforce.log:b2",
            "AppDiscovery.log:a3",
        ];
        assert_eq!(view.rows(&set), want);

        // Newer lines are appended at the end.
        append(&b, &cm("b3", "10:00:05"));
        let r = set.poll(&Progress::default()).unwrap();
        assert!(matches!(r, ReadResult::Appended { from_key: None, .. }));
        view.apply(&set, r);
        assert_eq!(view.rows(&set).last().unwrap(), "AppEnforce.log:b3");

        // An older line (before b3, but still after a3 of its own log) and an unfinished one
        // (provisional) of the other log.
        append(&a, &(cm("a4", "10:00:03") + "a5 unfinished"));
        let r = set.poll(&Progress::default()).unwrap();
        assert!(matches!(
            r,
            ReadResult::Appended {
                from_key: Some(_),
                ..
            }
        ));
        view.apply(&set, r);
        let rows = view.rows(&set);
        assert_eq!(
            rows[rows.len() - 4..],
            [
                "AppDiscovery.log:a3",
                "AppDiscovery.log:a4",
                "AppEnforce.log:b3",
                "AppDiscovery.log:a5 unfinished"
            ]
        );
        // The provisional line of a log stays while only the other log changes.
        append(&b, &cm("b4", "10:00:06"));
        read(&mut set, &mut view);
        let rows = view.rows(&set);
        assert_eq!(
            rows[rows.len() - 2..],
            ["AppEnforce.log:b4", "AppDiscovery.log:a5 unfinished"]
        );
        append(&a, " end\n");
        read(&mut set, &mut view);
        assert_eq!(view.committed, view.store.len());

        // Same as reading the final files at once.
        let mut fresh = LogSet::new(&[a.clone(), b.clone()], false);
        let mut all = View {
            store: LogStore::new(),
            committed: 0,
        };
        read(&mut fresh, &mut all);
        assert_eq!(view.rows(&set), all.rows(&fresh));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Different formats: the columns are united, extra columns keep their values; rotated
    /// parts of a log are still read; a truncated log makes the view read everything again.
    #[test]
    fn formats_rotated_parts_and_reread() {
        let dir = test_dir("formats");
        let iis = dir.join("u_ex261023.log");
        std::fs::write(
            &iis,
            "#Software: Microsoft Internet Information Services 10.0\n\
             #Fields: date time cs-method cs-uri-stem sc-status\n\
             2026-10-21 10:00:01 GET /a 200\n\
             2026-10-25 10:00:03 GET /b 404\n",
        )
        .unwrap();
        let log = dir.join("service.log");
        std::fs::write(dir.join("service.lo_"), cm("old", "09:00:00")).unwrap();
        std::fs::write(&log, cm("x", "10:00:02")).unwrap();
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(dir.join("service.lo_"))
            .unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
            .unwrap();
        drop(f);

        let mut set = LogSet::new(&[log.clone(), iis.clone()], false);
        set.defs = Arc::new(crate::logdef::load_from(&[], crate::logdef::SourceFilter::All).defs);
        set.merge_rotated = true;
        let mut view = View {
            store: LogStore::new(),
            committed: 0,
        };
        read(&mut set, &mut view);
        // The IIS times are UTC (shown in local time): days away from the CMTrace ones.
        assert_eq!(
            view.rows(&set),
            [
                "u_ex261023.log:GET /a",
                "service.lo_:old",
                "service.log:x",
                "u_ex261023.log:GET /b"
            ]
        );
        assert_eq!(set.file_sources(), [0, 0, 1]);
        // CMTrace columns first, then the IIS ones it does not have.
        let kinds: Vec<ColumnKind> = set.schema().columns.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds[..4],
            Schema::default()
                .columns
                .iter()
                .map(|c| c.kind)
                .collect::<Vec<_>>()[..]
        );
        let status = set
            .schema()
            .extra_names
            .iter()
            .position(|n| n == "sc-status")
            .unwrap();
        assert!(kinds.contains(&ColumnKind::Extra(status)));
        assert_eq!(view.store.extra(3, status), "404");
        assert_eq!(view.store.extra(1, status), "");
        assert!(set.mixed_formats());
        assert!(set.active_def().is_none());

        // The CMTrace log is replaced by a shorter one: everything is read again.
        std::fs::write(&log, cm("y", "10:00:00")).unwrap();
        let r = set.poll(&Progress::default()).unwrap();
        assert!(matches!(r, ReadResult::Reset { .. }));
        view.apply(&set, r);
        assert_eq!(
            view.rows(&set),
            [
                "u_ex261023.log:GET /a",
                "service.lo_:old",
                "service.log:y",
                "u_ex261023.log:GET /b"
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Loading time of logs merged vs one after another:
    /// `RUSTRACE_BENCH_FILES="a.log;b.log" cargo test --release bench_merge -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_merge() {
        let paths: Vec<PathBuf> = std::env::var("RUSTRACE_BENCH_FILES")
            .expect("RUSTRACE_BENCH_FILES")
            .split(';')
            .map(PathBuf::from)
            .collect();
        let defs = Arc::new(crate::logdef::load_from(&[], crate::logdef::SourceFilter::All).defs);
        let t = std::time::Instant::now();
        let mut separately = 0;
        for p in &paths {
            let mut set = LogSet::new(std::slice::from_ref(p), true);
            set.defs = defs.clone();
            if let ReadResult::Reset { committed, .. } = set.poll(&Progress::default()).unwrap() {
                separately += committed.len();
            }
        }
        let alone = t.elapsed();
        let t = std::time::Instant::now();
        let mut set = LogSet::new(&paths, true);
        set.defs = defs;
        let ReadResult::Reset { committed, .. } = set.poll(&Progress::default()).unwrap() else {
            panic!()
        };
        println!(
            "BENCH separately {alone:.2?} ({separately} entries), merged {:.2?} ({} entries)",
            t.elapsed(),
            committed.len()
        );
    }

    /// One log: the results of `LogSource` unchanged.
    #[test]
    fn single_log_passes_through() {
        let dir = test_dir("single");
        let p = dir.join("x.log");
        std::fs::write(&p, "b 2\na 1\npartial").unwrap();
        let mut set = LogSet::new(std::slice::from_ref(&p), false);
        let ReadResult::Reset {
            committed,
            provisional,
        } = set.poll(&Progress::default()).unwrap()
        else {
            panic!()
        };
        // Not reordered.
        assert_eq!((committed.message(0), committed.message(1)), ("b 2", "a 1"));
        assert_eq!(provisional.message(0), "partial");
        assert_eq!(set.files().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
