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

impl LogEntry {
    /// Value of an extra column (empty when the entry does not have it).
    pub fn extra(&self, i: usize) -> &str {
        self.extra.get(i).map_or("", String::as_str)
    }
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
        committed: Vec<LogEntry>,
        provisional: Vec<LogEntry>,
    },
    /// The file was read from the start (first open, truncation, rotation).
    Reset {
        committed: Vec<LogEntry>,
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

pub struct LogSource {
    pub path: PathBuf,
    offset: u64,
    encoding: Encoding,
    /// Bytes after the last line break (incomplete line).
    leftover: Vec<u8>,
    state: ParserState,
    /// Assign severity by keywords (error/fail/warn) to entries without a type.
    pub keyword_severity: bool,
    pub file_size: u64,
    /// How the format is chosen (used when the file is read from the start).
    pub format: FormatChoice,
    /// Available log definitions.
    pub defs: Arc<Vec<Arc<LogDef>>>,
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
            state: ParserState::new(),
            keyword_severity,
            file_size: 0,
            format: FormatChoice::Auto,
            defs: Arc::new(Vec::new()),
            format_decided: false,
        }
    }

    fn reset(&mut self) {
        self.offset = 0;
        self.encoding = Encoding::Utf8;
        self.leftover.clear();
        self.state = ParserState::new();
        self.format_decided = false;
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

    /// Reads new data from the file (in chunks). Called from a background thread.
    pub fn poll(&mut self, progress: &Progress) -> std::io::Result<PollResult> {
        let mut f = File::open(&self.path)?;
        let len = f.metadata()?.len();
        self.file_size = len;

        if len < self.offset {
            // The file was truncated or replaced by a new one.
            self.reset();
        }
        let is_reset = self.offset == 0;
        if len == self.offset && !is_reset {
            return Ok(PollResult::Unchanged);
        }
        progress.total.store(len - self.offset, Ordering::Relaxed);
        progress.done.store(0, Ordering::Relaxed);

        f.seek(SeekFrom::Start(self.offset))?;
        let mut committed = Vec::new();
        let mut first = is_reset;
        let mut buf = Vec::new();
        while self.offset < len {
            if progress.cancel.load(Ordering::Relaxed) {
                return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
            }
            buf.clear();
            let want = (len - self.offset).min(CHUNK);
            let n = (&mut f).take(want).read_to_end(&mut buf)?;
            if n == 0 {
                break; // the file got shorter in the meantime
            }
            self.offset += n as u64;
            progress.done.fetch_add(n as u64, Ordering::Relaxed);

            let mut data: &[u8] = &buf;
            if first {
                first = false;
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
            committed.reserve(text.len() / 120);
            for line in split_complete_lines(&text) {
                if let Some(e) = parse_line(&mut self.state, line, self.keyword_severity) {
                    committed.push(e);
                }
            }
        }

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
                assert_eq!(committed[0].message, "partial end");
                assert_eq!(committed[0].line_no, 3);
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
        assert_eq!(committed[0].message, "two\n   detail\n   more");
        assert_eq!(committed[0].line_no, 2);
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
        assert_eq!(committed[0].message, "GET /a");
        assert_eq!(committed[0].severity, Severity::Error);

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
        assert_eq!(committed[0].message, "2026-10-23 10:00:00, Info  DISM   x");
        std::fs::remove_dir_all(&dir).ok();
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
                assert_eq!(committed[0].message, "Grüße, žluť");
                assert_eq!(committed[1].severity, Severity::Error);
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
                assert_eq!(committed[n].message, "žltva");
                assert_eq!(committed[n].line_no, n + 1);
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
