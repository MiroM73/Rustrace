//! Reading and parsing log files.
//!
//! Supported formats:
//! * CMTrace / SCCM (ConfigMgr) – `<![LOG[message]LOG]!><time="..." date="..." component="..." type="1" thread="..." ...>`
//!   including multi-line messages,
//! * the older SMS format – `message $$<component><date time><thread=1234 (0x4D2)>`,
//! * plain text (every line = one entry),
//! * any layout described by a log definition (`logdef.rs`) – chosen by file name, by the
//!   first lines of the file, or by the user.
//!
//! Encoding: UTF-8 (with or without BOM), UTF-16 LE/BE (by BOM); if the file is not
//! valid UTF-8, Windows-1250 (Central European ANSI) is used.
//! The file is read in 8 MB chunks (in a background thread) with progress reporting.
//!
//! Reading is incremental – a refresh reads only the bytes added since the last
//! offset. If the file shrinks (rotation / truncation), it is read again from the start.
//!
//! Dates are stored as ISO `YYYY-MM-DD HH:MM:SS.mmm`; the UI formats them per language.

use crate::logdef::{self, DefLine, DefState, LogDef, Schema};
use crate::store::LogStore;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    /// Line number in the file (1-based) where the entry starts.
    pub line_no: usize,
    pub message: String,
    pub component: String,
    pub datetime: String,
    pub thread: String,
    pub severity: Severity,
    /// Values of the extra columns of a log definition (`Schema::extra_names`); empty otherwise.
    pub extra: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Encoding {
    Utf8,
    Ansi,
    Utf16Le,
    Utf16Be,
}

/// Result of one refresh.
pub enum PollResult {
    /// The file has not changed.
    Unchanged,
    /// New entries were appended at the end.
    Appended {
        committed: LogStore,
        provisional: Vec<LogEntry>,
    },
    /// The file was read from the start (first open, truncation, rotation).
    Reset {
        committed: LogStore,
        provisional: Vec<LogEntry>,
    },
}

#[derive(Clone)]
struct ParserState {
    /// Unfinished multi-line CMTrace entry (line, text).
    pending: Option<(usize, String)>,
    /// Number of the next line.
    next_line: usize,
    /// Columns of the log.
    schema: Schema,
    /// Log definition in use (`None` = built-in CMTrace / SMS / plain text parser).
    def: Option<DefState>,
    /// Last entry parsed with a definition – following unmatched lines may still be appended.
    pending_entry: Option<LogEntry>,
}

impl ParserState {
    fn new() -> Self {
        Self {
            pending: None,
            next_line: 1,
            schema: Schema::default(),
            def: None,
            pending_entry: None,
        }
    }
}

/// How the format of a file is chosen.
#[derive(Clone, Debug, Default)]
pub enum FormatChoice {
    /// A definition matching the file name, else by the first lines of the file, else built-in.
    #[default]
    Auto,
    /// Always the built-in CMTrace / SMS / plain text parser.
    Builtin,
    Def(Arc<LogDef>),
}

/// Loading progress – shared between the background thread and the UI.
#[derive(Default)]
pub struct Progress {
    /// Number of bytes to read in this round.
    pub total: AtomicU64,
    /// Number of bytes already processed.
    pub done: AtomicU64,
    /// Request to cancel loading.
    pub cancel: AtomicBool,
}

/// Read chunk size – the file is never loaded into memory all at once.
const CHUNK: u64 = 8 * 1024 * 1024;

/// Bytes from the start of the active file that are remembered; when the file no longer starts
/// with them it was rotated (renamed) or replaced.
const HEAD_LEN: usize = 4096;

/// One file of the log: the active file or one of its rotated older parts.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceFile {
    /// File name shown in the UI.
    pub name: String,
    /// Where the lines of this file are now (`None` = overwritten by a later rotation).
    pub path: Option<PathBuf>,
}

impl SourceFile {
    fn new(path: &Path) -> Self {
        Self {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: Some(path.to_path_buf()),
        }
    }
}

/// Rotated older parts of a log, oldest first: `service.lo_` (ConfigMgr renames the full log)
/// and `service-YYYYMMDD-HHMMSS.log` (newer ConfigMgr versions with more history files).
/// Sorted by modification time, then by name.
pub fn rotated_parts(active: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem), Some(ext)) = (
        active.parent(),
        active.file_stem().and_then(|s| s.to_str()),
        active.extension().and_then(|s| s.to_str()),
    ) else {
        return Vec::new();
    };
    let (stem, ext) = (stem.to_lowercase(), ext.to_lowercase());
    // "log" → "lo_"
    let rotated_ext = format!("{}_", &ext[..ext.len() - ext.chars().last().map_or(0, char::len_utf8)]);
    let is_part = |name: &str| {
        let name = name.to_lowercase();
        if name == format!("{stem}.{rotated_ext}") {
            return true;
        }
        // stem-YYYYMMDD-HHMMSS.ext
        let Some(rest) = name.strip_prefix(&format!("{stem}-")) else { return false };
        let Some(stamp) = rest.strip_suffix(&format!(".{ext}")) else { return false };
        let b = stamp.as_bytes();
        b.len() == 15
            && b[8] == b'-'
            && b.iter().enumerate().all(|(i, c)| i == 8 || c.is_ascii_digit())
    };
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut parts: Vec<(std::time::SystemTime, String, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let meta = e.metadata().ok()?;
            (meta.is_file() && is_part(&name)).then(|| {
                (meta.modified().unwrap_or(std::time::UNIX_EPOCH), name.to_lowercase(), e.path())
            })
        })
        .collect();
    parts.sort();
    parts.into_iter().map(|(_, _, p)| p).collect()
}

pub struct LogSource {
    pub path: PathBuf,
    offset: u64,
    encoding: Encoding,
    /// Bytes after the last line break (incomplete line).
    leftover: Vec<u8>,
    /// First bytes of the active file (up to `HEAD_LEN`).
    head: Vec<u8>,
    state: ParserState,
    /// Assign severity by keywords (error/fail/warn) to entries without a type.
    pub keyword_severity: bool,
    pub file_size: u64,
    /// How the format is chosen (used when the file is read from the start).
    pub format: FormatChoice,
    /// Available log definitions.
    pub defs: Arc<Vec<Arc<LogDef>>>,
    /// Reads of at least this many bytes keep the message text in a memory-mapped temporary
    /// file (`None` = always in memory).
    pub temp_file_threshold: Option<u64>,
    /// Also read the rotated older parts of the log (see `rotated_parts`) and keep the history
    /// when the active file is rotated while it is watched.
    pub merge_rotated: bool,
    /// Files the entries come from (`LogStore` file id = index); the last one is the active file.
    files: Vec<SourceFile>,
    /// The format for the current read has been chosen.
    format_decided: bool,
}

impl LogSource {
    pub fn new(path: impl AsRef<Path>, keyword_severity: bool) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            offset: 0,
            encoding: Encoding::Utf8,
            leftover: Vec::new(),
            head: Vec::new(),
            state: ParserState::new(),
            keyword_severity,
            file_size: 0,
            format: FormatChoice::Auto,
            defs: Arc::new(Vec::new()),
            temp_file_threshold: None,
            merge_rotated: false,
            files: Vec::new(),
            format_decided: false,
        }
    }

    fn reset(&mut self) {
        self.start_file();
        self.state = ParserState::new();
        self.files.clear();
        self.format_decided = false;
    }

    /// Reading state for the next file (the format and the columns stay).
    fn start_file(&mut self) {
        self.offset = 0;
        self.encoding = Encoding::Utf8;
        self.leftover.clear();
        self.head.clear();
        self.state.next_line = 1;
        self.state.pending = None;
        self.state.pending_entry = None;
    }

    /// Files the entries come from (index = file id of the entries); the last one is active.
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    fn active_id(&self) -> u16 {
        self.files.len().saturating_sub(1) as u16
    }

    /// Columns of the log (the built-in ones until a definition is chosen).
    pub fn schema(&self) -> &Schema {
        &self.state.schema
    }

    fn use_def(&mut self, def: Arc<LogDef>) {
        self.state.def = Some(DefState::new(def, &mut self.state.schema));
    }

    /// Chooses the format before the first line is parsed: the user's choice, a definition
    /// by file name, or – when `text` (the first lines) is available – by content.
    /// Without text and without a decision it is tried again with the next data.
    fn decide_format(&mut self, text: Option<&str>) {
        if self.format_decided {
            return;
        }
        match self.format.clone() {
            FormatChoice::Builtin => {}
            FormatChoice::Def(d) => self.use_def(d),
            FormatChoice::Auto => {
                if let Some(d) = logdef::by_file_name(&self.defs, &self.path) {
                    self.use_def(d);
                } else if let Some(text) = text {
                    let lines: Vec<&str> = text
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .take(30)
                        .collect();
                    // CMTrace / SMS content keeps the built-in parser.
                    let cmtrace = lines
                        .iter()
                        .any(|l| l.trim_start().starts_with(CM_START) || l.contains("$$<"));
                    if !cmtrace && let Some(d) = logdef::by_content(&self.defs, &lines) {
                        self.use_def(d);
                    }
                } else {
                    return;
                }
            }
        }
        self.format_decided = true;
    }

    pub fn encoding_name(&self) -> &'static str {
        match self.encoding {
            Encoding::Utf8 => "UTF-8",
            Encoding::Ansi => "ANSI (1250)",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
        }
    }

    /// The active file still starts with the remembered first bytes.
    fn head_matches(&self, f: &mut File) -> std::io::Result<bool> {
        if self.head.is_empty() {
            return Ok(true);
        }
        let mut buf = vec![0u8; self.head.len()];
        f.seek(SeekFrom::Start(0))?;
        let n = f.read(&mut buf)?;
        Ok(n == buf.len() && buf == self.head)
    }

    /// After the active file was rotated: the older part that now holds its lines (it starts
    /// with the same bytes and is at least as long as what was read).
    fn find_rotated(&self) -> Option<PathBuf> {
        rotated_parts(&self.path).into_iter().rev().find(|p| {
            let Ok(mut f) = File::open(p) else { return false };
            let long_enough = f.metadata().is_ok_and(|m| m.len() >= self.offset);
            long_enough && self.head_matches(&mut f).unwrap_or(false)
        })
    }

    /// Reads new data (in chunks). Called from a background thread.
    ///
    /// With `merge_rotated` the first read also reads the rotated older parts (oldest first),
    /// and when the active file is rotated while it is watched, the rest of the old file is read
    /// from its new name and the new active file is appended – nothing read so far is lost.
    pub fn poll(&mut self, progress: &Progress) -> std::io::Result<PollResult> {
        let mut f = File::open(&self.path)?;
        let len = f.metadata()?.len();

        let mut rotated = None;
        if self.offset > 0 && (len < self.offset || !self.head_matches(&mut f)?) {
            // The file was rotated, truncated or replaced by a new one.
            rotated = if self.merge_rotated { self.find_rotated() } else { None };
            if rotated.is_none() {
                self.reset();
            }
        }
        let is_reset = self.files.is_empty();
        let older = if is_reset && self.merge_rotated {
            rotated_parts(&self.path)
        } else {
            Vec::new()
        };
        let size = |p: &Path| std::fs::metadata(p).map_or(0, |m| m.len());
        let rest_of_rotated = rotated.as_deref().map_or(0, |p| size(p).saturating_sub(self.offset));
        let active_start = if rotated.is_some() { 0 } else { self.offset };
        let total = older.iter().map(|p| size(p)).sum::<u64>() + rest_of_rotated + len - active_start;
        if !is_reset && rotated.is_none() && len == self.offset {
            return Ok(PollResult::Unchanged);
        }
        progress.total.store(total, Ordering::Relaxed);
        progress.done.store(0, Ordering::Relaxed);

        let mut committed = if self.temp_file_threshold.is_some_and(|t| total >= t) {
            LogStore::new_mapped()
        } else {
            LogStore::new()
        };
        let mut reserved = false;
        if is_reset {
            for p in &older {
                self.files.push(SourceFile::new(p));
                let id = self.active_id();
                let mut pf = File::open(p)?;
                self.read_to_end(&mut pf, id, &mut committed, progress, &mut reserved)?;
                self.finish_file(&mut committed, id);
            }
            self.files.push(SourceFile::new(&self.path));
        }
        if let Some(rot) = rotated {
            // The rest of the old active file is now in the rotated file.
            let id = self.active_id();
            let mut rf = File::open(&rot)?;
            self.read_to_end(&mut rf, id, &mut committed, progress, &mut reserved)?;
            self.finish_file(&mut committed, id);
            // A file overwritten by this rotation (e.g. the previous .lo_) is gone.
            for file in &mut self.files {
                if file.path.as_deref() == Some(rot.as_path()) {
                    file.path = None;
                }
            }
            self.files[id as usize] = SourceFile::new(&rot);
            self.files.push(SourceFile::new(&self.path));
        }
        let id = self.active_id();
        self.read_to_end(&mut f, id, &mut committed, progress, &mut reserved)?;
        self.file_size = self.files.iter().filter_map(|s| s.path.as_deref()).map(size).sum();

        // Provisional entries: incomplete last line + unfinished multi-line entry.
        // The parser state is not changed – the next refresh replaces them with real ones.
        let tail = (!self.leftover.is_empty()).then(|| decode(&self.leftover, self.encoding));
        let tail = tail
            .as_deref()
            .map(|t| t.trim_end_matches(['\r', '\n']))
            .filter(|t| !t.is_empty());
        if let Some(t) = tail {
            self.decide_format(Some(t));
        }
        let mut tmp = self.state.clone();
        let mut provisional = Vec::new();
        if let Some(tail) = tail
            && let Some(e) = parse_line(&mut tmp, tail, self.keyword_severity)
        {
            provisional.push(e);
        }
        if let Some(e) = flush_pending(&mut tmp, self.keyword_severity) {
            provisional.push(e);
        }

        Ok(if is_reset {
            PollResult::Reset {
                committed,
                provisional,
            }
        } else {
            PollResult::Appended {
                committed,
                provisional,
            }
        })
    }

    /// End of a complete (older) file: its last line and an unfinished entry are final.
    fn finish_file(&mut self, committed: &mut LogStore, id: u16) {
        let tail = decode(&self.leftover, self.encoding);
        let tail = tail.trim_end_matches(['\r', '\n']);
        if !tail.is_empty() {
            self.decide_format(Some(tail));
            if let Some(e) = parse_line(&mut self.state, tail, self.keyword_severity) {
                committed.push(e, id);
            }
        }
        if let Some(e) = flush_pending(&mut self.state, self.keyword_severity) {
            committed.push(e, id);
        }
        self.start_file();
    }

    /// Reads `f` from `self.offset` to its current end into `committed` (entries of file `id`).
    fn read_to_end(
        &mut self,
        f: &mut File,
        id: u16,
        committed: &mut LogStore,
        progress: &Progress,
        reserved: &mut bool,
    ) -> std::io::Result<()> {
        let len = f.metadata()?.len();
        f.seek(SeekFrom::Start(self.offset))?;
        let mut first = self.offset == 0;
        let mut buf = Vec::new();
        while self.offset < len {
            if progress.cancel.load(Ordering::Relaxed) {
                return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
            }
            buf.clear();
            let want = (len - self.offset).min(CHUNK);
            let n = (&mut *f).take(want).read_to_end(&mut buf)?;
            if n == 0 {
                break; // the file got shorter in the meantime
            }
            self.offset += n as u64;
            progress.done.fetch_add(n as u64, Ordering::Relaxed);

            let mut data: &[u8] = &buf;
            if first {
                first = false;
                self.head = buf[..buf.len().min(HEAD_LEN)].to_vec();
                // BOM detection.
                if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
                    data = &data[3..];
                } else if data.starts_with(&[0xFF, 0xFE]) {
                    self.encoding = Encoding::Utf16Le;
                    data = &data[2..];
                } else if data.starts_with(&[0xFE, 0xFF]) {
                    self.encoding = Encoding::Utf16Be;
                    data = &data[2..];
                }
            }
            self.leftover.extend_from_slice(data);

            // Split complete lines from the incomplete tail.
            let cut = self.last_line_end();
            if cut == 0 {
                continue;
            }
            let complete: Vec<u8> = self.leftover.drain(..cut).collect();
            if self.encoding == Encoding::Utf8 && std::str::from_utf8(&complete).is_err() {
                // Not valid UTF-8 → read as ANSI (Windows-1250) from now on.
                self.encoding = Encoding::Ansi;
            }
            let text = decode(&complete, self.encoding);
            drop(complete);
            self.decide_format(Some(&text));
            for line in split_complete_lines(&text) {
                if let Some(e) = parse_line(&mut self.state, line, self.keyword_severity) {
                    committed.push(e, id);
                }
            }
            // After the first chunk: room for the whole read, estimated from that chunk, so the
            // storage does not grow (and copy) step by step.
            let done = progress.done.load(Ordering::Relaxed);
            let rest = progress.total.load(Ordering::Relaxed).saturating_sub(done);
            if !*reserved && rest > 0 && done > 0 && !committed.is_empty() {
                *reserved = true;
                let scale = rest as f64 / done as f64 * 1.05;
                committed.reserve(
                    (committed.len() as f64 * scale) as usize,
                    (committed.text_len() as f64 * scale) as usize,
                );
            }
        }
        Ok(())
    }

    /// Byte index just after the last line break in `leftover` (0 if none).
    fn last_line_end(&self) -> usize {
        let b = &self.leftover;
        match self.encoding {
            Encoding::Utf8 | Encoding::Ansi => {
                b.iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1)
            }
            Encoding::Utf16Le | Encoding::Utf16Be => {
                let nl: [u8; 2] = if self.encoding == Encoding::Utf16Le {
                    [0x0A, 0x00]
                } else {
                    [0x00, 0x0A]
                };
                let mut i = b.len() & !1;
                while i >= 2 {
                    if b[i - 2] == nl[0] && b[i - 1] == nl[1] {
                        return i;
                    }
                    i -= 2;
                }
                0
            }
        }
    }
}

fn decode(bytes: &[u8], enc: Encoding) -> String {
    match enc {
        Encoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        Encoding::Ansi => encoding_rs::WINDOWS_1250
            .decode_without_bom_handling(bytes)
            .0
            .into_owned(),
        Encoding::Utf16Le => encoding_rs::UTF_16LE
            .decode_without_bom_handling(bytes)
            .0
            .into_owned(),
        Encoding::Utf16Be => encoding_rs::UTF_16BE
            .decode_without_bom_handling(bytes)
            .0
            .into_owned(),
    }
}

/// Reads the given ranges of physical lines (1-based, inclusive) from a file, decoded the
/// same way as for viewing (BOM, UTF-8, ANSI fallback). Used by the export of original lines.
pub fn read_line_ranges(
    path: &Path,
    ranges: &[(usize, usize)],
) -> std::io::Result<Vec<Vec<String>>> {
    let bytes = std::fs::read(path)?;
    let (enc, body): (Encoding, &[u8]) = if let Some(b) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (Encoding::Utf8, b)
    } else if let Some(b) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        (Encoding::Utf16Le, b)
    } else if let Some(b) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (Encoding::Utf16Be, b)
    } else if std::str::from_utf8(&bytes).is_ok() {
        (Encoding::Utf8, &bytes)
    } else {
        (Encoding::Ansi, &bytes)
    };
    let text = decode(body, enc);
    drop(bytes);
    let body = text.strip_suffix('\n').unwrap_or(&text);
    let lines: Vec<&str> = body
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    Ok(ranges
        .iter()
        .map(|&(a, b)| {
            (a.max(1)..=b)
                .filter_map(|n| lines.get(n - 1))
                .map(|s| (*s).to_owned())
                .collect()
        })
        .collect())
}

/// Splits text ending with '\n' into lines (without '\r').
fn split_complete_lines(text: &str) -> impl Iterator<Item = &str> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let empty = text.is_empty();
    body.split('\n')
        .filter(move |_| !empty)
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
}

const CM_START: &str = "<![LOG[";
const CM_END: &str = "]LOG]!>";

fn parse_line(st: &mut ParserState, line: &str, kw: bool) -> Option<LogEntry> {
    if st.def.is_some() {
        return parse_def_line(st, line, kw);
    }
    let line_no = st.next_line;
    st.next_line += 1;

    if let Some((start_no, mut buf)) = st.pending.take() {
        buf.push('\n');
        buf.push_str(line);
        if buf.contains(CM_END) {
            return Some(parse_cmtrace(start_no, &buf, kw));
        }
        st.pending = Some((start_no, buf));
        return None;
    }

    let trimmed = line.trim_start();
    if trimmed.starts_with(CM_START) {
        if trimmed.contains(CM_END) {
            return Some(parse_cmtrace(line_no, trimmed, kw));
        }
        st.pending = Some((line_no, trimmed.to_owned()));
        return None;
    }

    if let Some(pos) = line.find("$$<") {
        return Some(parse_sms(line_no, line, pos, kw));
    }

    Some(plain_entry(line_no, line, kw))
}

/// One line of a log described by a definition. With `unmatched = append` an entry is
/// returned only when the next entry starts (or by `flush_pending` at the end of the data).
fn parse_def_line(st: &mut ParserState, line: &str, kw: bool) -> Option<LogEntry> {
    let line_no = st.next_line;
    st.next_line += 1;
    let ds = st.def.as_mut()?;
    let append = ds.appends_unmatched();
    match ds.parse(line, line_no, kw, &mut st.schema) {
        DefLine::Skip => None,
        DefLine::Entry(e) if append => st.pending_entry.replace(e),
        DefLine::Entry(e) => Some(e),
        DefLine::Unmatched => {
            if ds.skips_unmatched() {
                return None;
            }
            if append && let Some(p) = st.pending_entry.as_mut() {
                p.message.push('\n');
                p.message.push_str(line);
                return None;
            }
            Some(plain_entry(line_no, line, kw))
        }
    }
}

fn plain_entry(line_no: usize, line: &str, kw: bool) -> LogEntry {
    LogEntry {
        line_no,
        message: line.to_owned(),
        component: String::new(),
        datetime: String::new(),
        thread: String::new(),
        severity: keyword_sev(line, kw),
        extra: Vec::new(),
    }
}

fn flush_pending(st: &mut ParserState, kw: bool) -> Option<LogEntry> {
    if let Some(e) = st.pending_entry.take() {
        return Some(e);
    }
    let (no, buf) = st.pending.take()?;
    let msg = buf.strip_prefix(CM_START).unwrap_or(&buf).to_owned();
    Some(LogEntry {
        line_no: no,
        severity: keyword_sev(&msg, kw),
        message: msg,
        component: String::new(),
        datetime: String::new(),
        thread: String::new(),
        extra: Vec::new(),
    })
}

fn attr<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    // Look for ` key="` (preceded by a space or '<'), so "time" is not found inside another name.
    let pat = format!("{key}=\"");
    let mut from = 0;
    while let Some(rel) = s[from..].find(&pat) {
        let i = from + rel;
        let ok = i == 0 || matches!(s.as_bytes()[i - 1], b' ' | b'<' | b'\t');
        let vstart = i + pat.len();
        if ok {
            let vend = s[vstart..].find('"')?;
            return Some(&s[vstart..vstart + vend]);
        }
        from = vstart;
    }
    None
}

fn parse_cmtrace(line_no: usize, s: &str, kw: bool) -> LogEntry {
    let start = s.find(CM_START).map_or(0, |i| i + CM_START.len());
    let end = s.find(CM_END).unwrap_or(s.len());
    let message = s[start..end.max(start)].to_owned();
    let rest = if end < s.len() { &s[end + CM_END.len()..] } else { "" };

    let time = attr(rest, "time").unwrap_or("");
    let date = attr(rest, "date").unwrap_or("");
    let component = attr(rest, "component").unwrap_or("").to_owned();
    let thread = attr(rest, "thread").map(format_thread).unwrap_or_default();
    let severity = match attr(rest, "type") {
        Some("3") => Severity::Error,
        Some("2") => Severity::Warning,
        Some("1") => Severity::Info,
        _ => keyword_sev(&message, kw),
    };

    LogEntry {
        line_no,
        datetime: format_datetime(date, time),
        message,
        component,
        thread,
        severity,
        extra: Vec::new(),
    }
}

fn parse_sms(line_no: usize, line: &str, pos: usize, kw: bool) -> LogEntry {
    let message = line[..pos].trim_end().to_owned();
    let rest = &line[pos + 2..];
    // Parts in angle brackets: <component><date time><thread=...>
    let parts: Vec<&str> = rest
        .split('<')
        .filter_map(|p| p.split_once('>').map(|(a, _)| a))
        .collect();
    let component = parts.first().copied().unwrap_or("").to_owned();
    let datetime = parts
        .get(1)
        .map(|dt| {
            let mut it = dt.splitn(2, ' ');
            let d = it.next().unwrap_or("");
            let t = it.next().unwrap_or("");
            format_datetime(d, t)
        })
        .unwrap_or_default();
    let thread = parts
        .get(2)
        .map(|t| t.trim_start_matches("thread=").to_owned())
        .unwrap_or_default();
    LogEntry {
        line_no,
        severity: keyword_sev(&message, kw),
        message,
        component,
        datetime,
        thread,
        extra: Vec::new(),
    }
}

pub(crate) fn keyword_sev(s: &str, enabled: bool) -> Severity {
    if !enabled {
        return Severity::Info;
    }
    let l = s.to_lowercase();
    if l.contains("error") || l.contains("fail") {
        Severity::Error
    } else if l.contains("warn") {
        Severity::Warning
    } else {
        Severity::Info
    }
}

/// "1234" → "1234 (0x04D2)"
fn format_thread(t: &str) -> String {
    match t.trim().parse::<u64>() {
        Ok(n) => format!("{n} (0x{n:04X})"),
        Err(_) => t.to_owned(),
    }
}

/// CMTrace: date="10-23-2026", time="13:22:01.123-120" → "2026-10-23 13:22:01.123"
fn format_datetime(date: &str, time: &str) -> String {
    // Remove the time zone (+/- minutes) after the time.
    let t = match time.find(['+', '-']) {
        Some(i) if i > 0 => &time[..i],
        _ => time,
    };
    // Cut to milliseconds (ConfigMgr sometimes writes 7 decimal places).
    let t = match t.find('.') {
        Some(i) if t.len() > i + 4 => &t[..i + 4],
        _ => t,
    };
    let d: Vec<&str> = date.split(['-', '/']).collect();
    let d = if d.len() == 3 && d[2].len() == 4 {
        // MM-DD-YYYY → YYYY-MM-DD (zero-padded)
        format!("{}-{:0>2}-{:0>2}", d[2], d[0], d[1])
    } else {
        date.to_owned()
    };
    match (d.is_empty(), t.is_empty()) {
        (true, _) => t.to_owned(),
        (_, true) => d,
        _ => format!("{d} {t}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn parse_all(text: &str) -> Vec<LogEntry> {
        let mut st = ParserState::new();
        split_complete_lines(text)
            .filter_map(|l| parse_line(&mut st, l, true))
            .collect()
    }

    #[test]
    fn cmtrace_single_and_multiline() {
        let t = "<![LOG[Hello]LOG]!><time=\"13:22:01.1234567-120\" date=\"10-23-2026\" component=\"CcmExec\" context=\"\" type=\"1\" thread=\"1234\" file=\"x.cpp:1\">\n\
                 <![LOG[Line one\nline two]LOG]!><time=\"13:22:02.000+000\" date=\"10-23-2026\" component=\"AppEnforce\" context=\"\" type=\"3\" thread=\"5\" file=\"\">\n";
        let e = parse_all(t);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].message, "Hello");
        assert_eq!(e[0].component, "CcmExec");
        assert_eq!(e[0].datetime, "2026-10-23 13:22:01.123");
        assert_eq!(e[0].thread, "1234 (0x04D2)");
        assert_eq!(e[0].severity, Severity::Info);
        assert_eq!(e[1].message, "Line one\nline two");
        assert_eq!(e[1].severity, Severity::Error);
        assert_eq!(e[1].line_no, 2);
    }

    #[test]
    fn sms_and_plain() {
        let t = "Some failure happened  $$<SMS_EXEC><10-23-2026 13:22:01.123-120><thread=4420 (0x1144)>\nplain warning text\nok\n";
        let e = parse_all(t);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].component, "SMS_EXEC");
        assert_eq!(e[0].severity, Severity::Error);
        assert_eq!(e[0].thread, "4420 (0x1144)");
        assert_eq!(e[1].severity, Severity::Warning);
        assert_eq!(e[2].severity, Severity::Info);
    }

    #[test]
    fn incremental_tail_and_truncate() {
        let dir = std::env::temp_dir().join(format!("rustrace_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.log");
        std::fs::write(&p, "a\nb\npartial").unwrap();
        let mut s = LogSource::new(&p, true);
        match s.poll(&Progress::default()).unwrap() {
            PollResult::Reset { committed, provisional } => {
                assert_eq!(committed.len(), 2);
                assert_eq!(provisional.len(), 1);
                assert_eq!(provisional[0].message, "partial");
            }
            _ => panic!(),
        }
        assert!(matches!(s.poll(&Progress::default()).unwrap(), PollResult::Unchanged));
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b" end\nc\n").unwrap();
        match s.poll(&Progress::default()).unwrap() {
            PollResult::Appended { committed, provisional } => {
                assert_eq!(committed.len(), 2);
                assert_eq!(committed.at(0).message, "partial end");
                assert_eq!(committed.at(0).line_no, 3);
                assert!(provisional.is_empty());
            }
            _ => panic!(),
        }
        std::fs::write(&p, "new\n").unwrap();
        assert!(matches!(s.poll(&Progress::default()).unwrap(), PollResult::Reset { .. }));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Log definitions: chosen by file name or by content, unmatched lines appended to the
    /// previous entry even across refreshes, CMTrace content keeps the built-in parser.
    #[test]
    fn log_definitions_in_files() {
        let dir = std::env::temp_dir().join(format!("rustrace_def_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let defs = Arc::new(logdef::load_from(&[]).defs);
        let open = |name: &str, text: &str| {
            let p = dir.join(name);
            std::fs::write(&p, text).unwrap();
            let mut s = LogSource::new(&p, true);
            s.defs = defs.clone();
            s
        };

        // By file name; the last entry stays provisional until the next entry starts.
        let mut s = open(
            "CBS.log",
            "2026-10-23 10:00:00, Info                  CBS    one\n\
             2026-10-23 10:00:01, Error                 CBS    two\n   detail\n",
        );
        let PollResult::Reset { committed, provisional } = s.poll(&Progress::default()).unwrap() else {
            panic!()
        };
        assert_eq!(s.schema().name, "Windows CBS log");
        assert_eq!(committed.len(), 1);
        assert_eq!(provisional[0].message, "two\n   detail");
        assert_eq!(provisional[0].severity, Severity::Error);
        let mut f = std::fs::OpenOptions::new().append(true).open(&s.path).unwrap();
        f.write_all(b"   more\n2026-10-23 10:00:02, Info                  CBS    three\n").unwrap();
        let PollResult::Appended { committed, provisional } = s.poll(&Progress::default()).unwrap()
        else {
            panic!()
        };
        assert_eq!(committed.at(0).message, "two\n   detail\n   more");
        assert_eq!(committed.at(0).line_no, 2);
        assert_eq!(provisional[0].message, "three");
        assert_eq!(provisional[0].line_no, 5);

        // By content (the name matches no definition).
        let mut s = open(
            "web-2026.txt",
            "#Software: Microsoft Internet Information Services 10.0\n\
             #Fields: date time cs-method cs-uri-stem sc-status\n\
             2026-10-23 08:00:01 GET /a 503\n",
        );
        let PollResult::Reset { committed, .. } = s.poll(&Progress::default()).unwrap() else {
            panic!()
        };
        assert_eq!(s.schema().name, "IIS web log (W3C)");
        assert_eq!(committed.at(0).message, "GET /a");
        assert_eq!(committed.at(0).severity, Severity::Error);

        // CMTrace content with a matching definition name pattern only by content → built-in.
        let mut s = open("other.log", "<![LOG[Hello]LOG]!><time=\"10:00:00.000+000\" date=\"10-23-2026\" component=\"C\" context=\"\" type=\"1\" thread=\"1\" file=\"\">\n");
        s.poll(&Progress::default()).unwrap();
        assert_eq!(s.schema().name, "");

        // The user's choice wins over the file name.
        let mut s = open("dism.log", "2026-10-23 10:00:00, Info  DISM   x\n");
        s.format = FormatChoice::Builtin;
        let PollResult::Reset { committed, .. } = s.poll(&Progress::default()).unwrap() else {
            panic!()
        };
        assert_eq!(s.schema().name, "");
        assert_eq!(committed.at(0).message, "2026-10-23 10:00:00, Info  DISM   x");
        std::fs::remove_dir_all(&dir).ok();
    }

    fn messages(s: &LogStore) -> Vec<(u16, usize, &str)> {
        (0..s.len()).map(|i| (s.file(i), s.line_no(i), s.message(i))).collect()
    }

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rustrace_{name}_{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn set_age(p: &Path, secs_ago: u64) {
        let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs_ago))
            .unwrap();
    }

    #[test]
    fn rotated_parts_by_name_and_age() {
        let dir = test_dir("parts");
        for (name, age) in [
            ("service.lo_", 100),
            ("Service-20261023-101500.log", 200),
            ("service.log", 0),
            ("service2.log", 50),
            ("service-2026.log", 50),
            ("service-20261023-101500.txt", 50),
            ("other.lo_", 50),
        ] {
            std::fs::write(dir.join(name), "x\n").unwrap();
            set_age(&dir.join(name), age);
        }
        let names: Vec<String> = rotated_parts(&dir.join("service.log"))
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["Service-20261023-101500.log", "service.lo_"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Opening a log reads its rotated parts first (oldest first), each with its own line numbers.
    #[test]
    fn merged_rotated_parts_on_open() {
        let dir = test_dir("merge");
        std::fs::write(dir.join("service-20261023-101500.log"), "h1\nh2").unwrap(); // no final newline
        std::fs::write(dir.join("service.lo_"), "r1\n").unwrap();
        std::fs::write(dir.join("service.log"), "a1\na2\n").unwrap();
        set_age(&dir.join("service-20261023-101500.log"), 200);
        set_age(&dir.join("service.lo_"), 100);

        let mut s = LogSource::new(dir.join("service.log"), false);
        s.merge_rotated = true;
        let PollResult::Reset { committed, provisional } = s.poll(&Progress::default()).unwrap() else {
            panic!()
        };
        assert!(provisional.is_empty());
        assert_eq!(
            messages(&committed),
            [(0, 1, "h1"), (0, 2, "h2"), (1, 1, "r1"), (2, 1, "a1"), (2, 2, "a2")]
        );
        let names: Vec<&str> = s.files().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["service-20261023-101500.log", "service.lo_", "service.log"]);

        // Without merging only the active file is read.
        let mut s = LogSource::new(dir.join("service.log"), false);
        let PollResult::Reset { committed, .. } = s.poll(&Progress::default()).unwrap() else {
            panic!()
        };
        assert_eq!(messages(&committed), [(0, 1, "a1"), (0, 2, "a2")]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Rotation while the log is watched: the unread rest of the old file is read from its new
    /// name, the new file is appended, and a file overwritten by the next rotation is marked.
    #[test]
    fn rotation_while_watching() {
        let dir = test_dir("rotate");
        let log = dir.join("service.log");
        let lo = dir.join("service.lo_");
        std::fs::write(&log, "a\nb\npart").unwrap();
        let mut s = LogSource::new(&log, false);
        s.merge_rotated = true;
        let PollResult::Reset { committed, provisional } = s.poll(&Progress::default()).unwrap() else {
            panic!()
        };
        assert_eq!(committed.len(), 2);
        assert_eq!(provisional[0].message, "part");

        // The writer finishes the line, rotates, and continues in a new file.
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        f.write_all(b" end\nc\n").unwrap();
        drop(f);
        std::fs::rename(&log, &lo).unwrap();
        std::fs::write(&log, "x\ny\n").unwrap();
        let PollResult::Appended { committed, provisional } = s.poll(&Progress::default()).unwrap()
        else {
            panic!("rotation must not reset the log")
        };
        assert!(provisional.is_empty());
        assert_eq!(messages(&committed), [(0, 3, "part end"), (0, 4, "c"), (1, 1, "x"), (1, 2, "y")]);
        let files: Vec<(&str, bool)> = s.files().iter().map(|f| (f.name.as_str(), f.path.is_some())).collect();
        assert_eq!(files, [("service.lo_", true), ("service.log", true)]);

        // Next rotation overwrites service.lo_ – its old lines are no longer in any file.
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        f.write_all(b"z\n").unwrap();
        drop(f);
        std::fs::rename(&log, &lo).unwrap();
        std::fs::write(&log, "q\n").unwrap();
        let PollResult::Appended { committed, .. } = s.poll(&Progress::default()).unwrap() else {
            panic!()
        };
        assert_eq!(messages(&committed), [(1, 3, "z"), (2, 1, "q")]);
        let files: Vec<(&str, bool)> = s.files().iter().map(|f| (f.name.as_str(), f.path.is_some())).collect();
        assert_eq!(files, [("service.lo_", false), ("service.lo_", true), ("service.log", true)]);
        assert!(matches!(s.poll(&Progress::default()).unwrap(), PollResult::Unchanged));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A file replaced by different content of at least the same size is read again.
    #[test]
    fn replaced_file_is_read_again() {
        let dir = test_dir("replace");
        let log = dir.join("x.log");
        std::fs::write(&log, "first\n").unwrap();
        let mut s = LogSource::new(&log, false);
        s.poll(&Progress::default()).unwrap();
        std::fs::write(&log, "second line\n").unwrap();
        let PollResult::Reset { committed, .. } = s.poll(&Progress::default()).unwrap() else {
            panic!("a replaced file must be read from the start")
        };
        assert_eq!(messages(&committed), [(0, 1, "second line")]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// (private bytes, working set, peak working set) of this process in MB.
    #[cfg(windows)]
    fn mem_mb() -> (f64, f64, f64) {
        #[repr(C)]
        #[derive(Default)]
        struct Counters {
            cb: u32,
            page_faults: u32,
            peak_ws: usize,
            ws: usize,
            q1: usize,
            q2: usize,
            q3: usize,
            q4: usize,
            pagefile: usize,
            peak_pagefile: usize,
            private: usize,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> isize;
            fn K32GetProcessMemoryInfo(p: isize, c: *mut Counters, cb: u32) -> i32;
        }
        let mut c = Counters {
            cb: std::mem::size_of::<Counters>() as u32,
            ..Default::default()
        };
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        let mb = |b: usize| b as f64 / 1048576.0;
        (mb(c.private), mb(c.ws), mb(c.peak_ws))
    }

    /// Measures loading, search and random access on a big file:
    /// `RUSTRACE_BENCH_FILE=<log> cargo test --release bench_large_file -- --ignored --nocapture`
    /// (`RUSTRACE_BENCH_MAPPED=1` keeps the text in a memory-mapped temporary file).
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn bench_large_file() {
        let path = std::env::var("RUSTRACE_BENCH_FILE").expect("RUSTRACE_BENCH_FILE");
        let mapped = std::env::var("RUSTRACE_BENCH_MAPPED").is_ok_and(|v| v == "1");
        let before = mem_mb();
        let t = std::time::Instant::now();
        let mut s = LogSource::new(&path, true);
        s.temp_file_threshold = mapped.then_some(0);
        let PollResult::Reset { committed: entries, .. } = s.poll(&Progress::default()).unwrap() else {
            panic!()
        };
        let load = t.elapsed();
        let after = mem_mb();

        let re = regex::RegexBuilder::new("error 0x8007").case_insensitive(true).build().unwrap();
        let t = std::time::Instant::now();
        let hits = (0..entries.len())
            .filter(|&i| {
                let e = entries.at(i);
                re.is_match(e.message) || re.is_match(e.component)
            })
            .count();
        let search = t.elapsed();

        let t = std::time::Instant::now();
        let mut x: u64 = 0x2545F4914F6CDD1D;
        let mut sum = 0usize;
        for _ in 0..1_000_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let e = entries.at((x % entries.len() as u64) as usize);
            sum += e.message.lines().next().unwrap_or("").len() + e.component.len() + e.datetime().len();
        }
        let random = t.elapsed();
        println!(
            "BENCH mapped={} entries={} load={:.2?} search={:.2?} (hits {hits}) random1M={:.2?} (sum {sum})",
            entries.is_mapped(),
            entries.len(),
            load,
            search,
            random
        );
        println!(
            "BENCH private {:.0} -> {:.0} MB, working set {:.0} -> {:.0} MB, peak working set {:.0} MB",
            before.0, after.0, before.1, after.1, after.2
        );
    }

    #[test]
    fn utf16le() {
        let dir = std::env::temp_dir().join(format!("rustrace_test16_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("u.log");
        let mut bytes = vec![0xFF, 0xFE];
        for u in "Grüße, žluť\r\nan error\r\n".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        std::fs::write(&p, bytes).unwrap();
        let mut s = LogSource::new(&p, true);
        match s.poll(&Progress::default()).unwrap() {
            PollResult::Reset { committed, .. } => {
                assert_eq!(committed.len(), 2);
                assert_eq!(committed.at(0).message, "Grüße, žluť");
                assert_eq!(committed.at(1).severity, Severity::Error);
            }
            _ => panic!(),
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn chunked_large_ansi_and_cancel() {
        let dir = std::env::temp_dir().join(format!("rustrace_big_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("big.log");
        // ~20 MB (several 8 MB chunks), with an ANSI line (0x9E = ž in Windows-1250).
        let line = "<![LOG[Some fairly long log message for chunk testing 1234567890]LOG]!><time=\"13:00:00.000-120\" date=\"10-23-2026\" component=\"X\" context=\"\" type=\"1\" thread=\"1\" file=\"\">\n";
        let n = 20 * 1024 * 1024 / line.len();
        let mut data = Vec::with_capacity(n * line.len() + 16);
        for _ in 0..n {
            data.extend_from_slice(line.as_bytes());
        }
        data.extend_from_slice(b"\x9Eltva\n");
        std::fs::write(&p, &data).unwrap();

        let prog = Progress::default();
        let mut s = LogSource::new(&p, true);
        match s.poll(&prog).unwrap() {
            PollResult::Reset { committed, .. } => {
                assert_eq!(committed.len(), n + 1);
                assert_eq!(committed.at(n).message, "žltva");
                assert_eq!(committed.at(n).line_no, n + 1);
            }
            _ => panic!(),
        }
        assert_eq!(prog.done.load(Ordering::Relaxed), data.len() as u64);
        assert_eq!(s.encoding_name(), "ANSI (1250)");

        let prog = Progress::default();
        prog.cancel.store(true, Ordering::Relaxed);
        let mut s = LogSource::new(&p, true);
        assert_eq!(s.poll(&prog).err().unwrap().kind(), std::io::ErrorKind::Interrupted);
        std::fs::remove_dir_all(&dir).ok();
    }
}
