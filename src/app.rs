use crate::config::{Config, HighlightRule, HighlightScope, IntervalUnit};
use crate::colfilter::ColumnFilter;
use crate::errcodes::{self, CodeInfo};
use crate::export::{self, ExportFormat, ReadableRow};
use crate::i18n;
use crate::logdef::{self, Column as LogColumn, ColumnKind, LogDef, Role, Schema};
use crate::parser::{FormatChoice, LogSource, PollResult, Progress, Severity, SourceFile};
use crate::store::{iso_to_ms, ms_to_iso, EntryRef, LogStore};
use crate::t;
use eframe::egui::{
    self, text::LayoutJob, Align, Color32, FontId, Key, Layout, Modifiers, RichText, Sense,
    TextFormat, TextStyle,
};
use egui_extras::{Column, TableBuilder};
use regex::{Regex, RegexBuilder};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// Max. number of message characters shown in the table (the full message is in the details pane).
const MAX_CELL_CHARS: usize = 2000;

const RED: Color32 = Color32::from_rgb(220, 60, 60);
const ORANGE: Color32 = Color32::from_rgb(230, 140, 0);

fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn compile_pattern(pattern: &str, is_regex: bool, case_sensitive: bool) -> Result<Regex, String> {
    let p = if is_regex {
        pattern.to_owned()
    } else {
        regex::escape(pattern)
    };
    RegexBuilder::new(&p)
        .case_insensitive(!case_sensitive)
        .size_limit(20 << 20)
        .build()
        .map_err(|e| e.to_string())
}

struct CompiledRule {
    regex: Regex,
    rule: HighlightRule,
}

/// A highlighted piece of text.
struct Span {
    start: usize,
    end: usize,
    fg: Option<Color32>,
    bg: Option<Color32>,
}

/// File reading in progress in a background thread.
struct Loading {
    rx: mpsc::Receiver<(LogSource, std::io::Result<PollResult>)>,
    progress: Arc<Progress>,
    started: Instant,
}

enum RowAction {
    ExplainCode(u32),
    CopyMessage(usize),
    CopyRow(usize),
    RuleFromComponent(usize),
    SearchComponent(usize),
}

pub struct App {
    cfg: Config,
    cfg_dirty: bool,
    last_save: Instant,

    ctx: egui::Context,
    /// While loading, the source is moved into the reader thread (then it is `None`).
    source: Option<LogSource>,
    loading: Option<Loading>,
    cur_path: Option<PathBuf>,
    encoding_name: &'static str,
    file_size: u64,
    /// The file is added to recent files only after it has been read successfully.
    pending_recent: Option<String>,
    entries: LogStore,
    /// Number of "final" entries; provisional ones follow (incomplete last line).
    committed_len: usize,
    /// Indexes of the displayed entries (after filtering).
    filtered: Vec<usize>,
    /// For every entry: does it match the search?
    is_match: Vec<bool>,
    match_count: usize,

    last_refresh: Option<Instant>,
    last_refresh_wall: Option<chrono::DateTime<chrono::Local>>,
    status_error: Option<String>,
    status_info: Option<(String, Instant)>,

    search_text: String,
    search_regex: bool,
    search_case: bool,
    filter_mode: bool,
    search_re: Option<Regex>,
    search_err: Option<String>,
    show_sev: [bool; 3], // info, warning, error

    selected: Option<usize>,
    scroll_to: Option<usize>,

    rules: Vec<CompiledRule>,
    rule_errors: Vec<Option<String>>,

    show_rules: bool,
    show_colors: bool,
    show_help: bool,
    focus_search: bool,
    title: String,

    /// Error codes found in the selected row (entry index, codes).
    code_cache: Option<(usize, Vec<CodeInfo>)>,
    show_lookup: bool,
    lookup_input: String,
    lookup_focus: bool,
    lookup_cache: Option<(String, Option<CodeInfo>)>,

    /// Date pattern of the current language, e.g. "{d}.{m}.{y}".
    date_format: String,
    /// Code of the language actually in use.
    lang_code: String,
    new_lang_code: String,

    /// The dark theme is active in this frame.
    dark: bool,
    /// Which theme's colors are edited in the rules/colors windows.
    edit_dark: bool,

    /// Change counters – used to know when cached export hits are stale.
    data_gen: u64,
    rules_gen: u64,
    search_gen: u64,
    show_export: bool,
    /// Cached highlighted entry indexes for the export: (key, hits).
    export_hits: Option<(ExportKey, Vec<usize>)>,
    /// Export running in a background thread; the result is a status message.
    export_job: Option<mpsc::Receiver<Result<String, String>>>,
    /// Newest timestamp in the log, cached per data generation.
    latest_ts: Option<(u64, Option<String>)>,
    /// The mouse pointer is over the log text (table or message in the details pane) in this frame.
    hover_log: bool,

    /// Loaded log definitions (`logdef\*.def`) and errors of files that could not be used.
    defs: Arc<Vec<Arc<LogDef>>>,
    def_errors: Vec<String>,
    /// Format chosen for the current file (View → Log format); `Auto` for every newly opened file.
    format_choice: FormatChoice,
    /// The log definition the current file is read with (`None` = built-in parser).
    active_def: Option<Arc<LogDef>>,
    /// Columns of the current log.
    schema: Schema,
    /// Texts of the column filters (filter row under the headers) and their compiled form.
    col_filter_text: HashMap<ColumnKind, String>,
    col_filters: Vec<(ColumnKind, ColumnFilter)>,
    /// Time range filter: texts as entered, parsed bounds (ms) and which text is invalid.
    time_from: String,
    time_to: String,
    time_range: (Option<i64>, Option<i64>),
    time_invalid: (bool, bool),
    /// Files the log is made of (rotated parts + active file; index = file id of the entries)
    /// and their labels for the File column.
    files: Vec<SourceFile>,
    file_labels: Vec<String>,
    /// Number of oldest entries removed because of the memory limit (since the last full read).
    dropped: usize,
    /// Horizontal scroll offset of the table and its natural width (measured every frame).
    table_scroll_x: f32,
    table_width: f32,
}

/// Limits of Ctrl + mouse wheel zoom.
const FONT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 6.0..=48.0;
const UI_ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.5..=3.0;
/// Minimum width of the last table column (with horizontal scrolling it is never narrower).
const LAST_COLUMN_MIN: f32 = 150.0;

/// What will be exported with the current settings.
struct ExportPlan {
    /// Highlighted entries after the time filter.
    hits: Vec<usize>,
    blocks: Vec<(usize, usize)>,
    /// The "since" date/time could not be parsed.
    since_invalid: bool,
}

/// Everything the list of highlighted rows depends on.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ExportKey {
    data_gen: u64,
    rules_gen: u64,
    search_gen: u64,
    len: usize,
    rules: bool,
    errors: bool,
    warnings: bool,
    search: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<String>) -> Self {
        let (mut cfg, existed) = Config::load();
        let lang_code = i18n::set_language(&cfg.language);
        if !existed {
            // First run: example rules named in the chosen language.
            cfg.rules = Config::default_rules();
        }
        let mut app = Self {
            cfg,
            cfg_dirty: false,
            last_save: Instant::now(),
            ctx: cc.egui_ctx.clone(),
            source: None,
            loading: None,
            cur_path: None,
            encoding_name: "",
            file_size: 0,
            pending_recent: None,
            entries: LogStore::new(),
            committed_len: 0,
            filtered: Vec::new(),
            is_match: Vec::new(),
            match_count: 0,
            last_refresh: None,
            last_refresh_wall: None,
            status_error: None,
            status_info: None,
            search_text: String::new(),
            search_regex: false,
            search_case: false,
            filter_mode: false,
            search_re: None,
            search_err: None,
            show_sev: [true; 3],
            selected: None,
            scroll_to: None,
            rules: Vec::new(),
            rule_errors: Vec::new(),
            show_rules: false,
            show_colors: false,
            show_help: false,
            focus_search: false,
            title: String::new(),
            code_cache: None,
            show_lookup: false,
            lookup_input: String::new(),
            lookup_focus: false,
            lookup_cache: None,
            date_format: t!("format.date"),
            lang_code,
            new_lang_code: String::new(),
            dark: cc.egui_ctx.global_style().visuals.dark_mode,
            edit_dark: cc.egui_ctx.global_style().visuals.dark_mode,
            data_gen: 0,
            rules_gen: 0,
            search_gen: 0,
            show_export: false,
            export_hits: None,
            export_job: None,
            latest_ts: None,
            hover_log: false,
            defs: Arc::new(Vec::new()),
            def_errors: Vec::new(),
            format_choice: FormatChoice::Auto,
            active_def: None,
            schema: Schema::default(),
            col_filter_text: HashMap::new(),
            col_filters: Vec::new(),
            time_from: String::new(),
            time_to: String::new(),
            time_range: (None, None),
            time_invalid: (false, false),
            files: Vec::new(),
            file_labels: Vec::new(),
            dropped: 0,
            table_scroll_x: 0.0,
            table_width: 0.0,
        };
        app.load_defs();
        app.apply_theme(&cc.egui_ctx);
        let zoom = app.cfg.ui_zoom.clamp(*UI_ZOOM_RANGE.start(), *UI_ZOOM_RANGE.end());
        cc.egui_ctx.set_zoom_factor(zoom);
        app.recompile_rules();
        if let Some(p) = path {
            app.open_file(&p);
        }
        app
    }

    // ------------------------------------------------------------------
    // Logic
    // ------------------------------------------------------------------

    fn apply_theme(&self, ctx: &egui::Context) {
        let pref = match self.cfg.theme.as_str() {
            "dark" => egui::ThemePreference::Dark,
            "light" => egui::ThemePreference::Light,
            _ => egui::ThemePreference::System,
        };
        ctx.set_theme(pref);
    }

    /// Ctrl + mouse wheel: over the log text it changes the log font size, anywhere else it zooms
    /// the whole user interface. Also remembers the UI zoom changed with Ctrl+plus / minus / 0.
    fn handle_zoom(&mut self, ctx: &egui::Context) {
        let delta = ctx.input(|i| i.zoom_delta());
        if delta != 1.0 {
            if self.hover_log {
                self.cfg.font_size = (self.cfg.font_size * delta)
                    .clamp(*FONT_SIZE_RANGE.start(), *FONT_SIZE_RANGE.end());
            } else {
                let zoom = (ctx.zoom_factor() * delta)
                    .clamp(*UI_ZOOM_RANGE.start(), *UI_ZOOM_RANGE.end());
                ctx.set_zoom_factor(zoom);
            }
            self.cfg_dirty = true;
        }
        if (ctx.zoom_factor() - self.cfg.ui_zoom).abs() > 0.001 {
            self.cfg.ui_zoom = ctx.zoom_factor();
            self.cfg_dirty = true;
        }
    }

    /// Cycles the theme: system → light → dark → system.
    fn cycle_theme(&mut self, ctx: &egui::Context) {
        self.cfg.theme = match self.cfg.theme.as_str() {
            "system" => "light",
            "light" => "dark",
            _ => "system",
        }
        .to_owned();
        self.apply_theme(ctx);
        self.cfg_dirty = true;
    }

    /// Switches the UI language ("" = system language) and clears translated caches.
    fn set_language(&mut self, code: &str, reload_files: bool) {
        self.cfg.language = code.to_owned();
        self.lang_code = if reload_files {
            i18n::reload(code)
        } else {
            i18n::set_language(code)
        };
        self.date_format = t!("format.date");
        self.code_cache = None;
        self.lookup_cache = None;
        self.cfg_dirty = true;
        // The date order of the time range texts depends on the language.
        self.apply_time_range();
        let files = std::mem::take(&mut self.files);
        self.set_files(files);
    }

    fn recompile_rules(&mut self) {
        self.rules_gen += 1;
        self.rules.clear();
        self.rule_errors.clear();
        for r in &self.cfg.rules {
            if r.pattern.is_empty() {
                self.rule_errors.push(None);
                continue;
            }
            match compile_pattern(&r.pattern, r.is_regex, r.case_sensitive) {
                Ok(re) => {
                    self.rule_errors.push(None);
                    if r.enabled {
                        self.rules.push(CompiledRule {
                            regex: re,
                            rule: r.clone(),
                        });
                    }
                }
                Err(e) => self.rule_errors.push(Some(e)),
            }
        }
    }

    fn recompile_search(&mut self) {
        self.search_gen += 1;
        self.search_err = None;
        self.search_re = None;
        if !self.search_text.is_empty() {
            match compile_pattern(&self.search_text, self.search_regex, self.search_case) {
                Ok(re) => self.search_re = Some(re),
                Err(e) => self.search_err = Some(e),
            }
        }
        self.refilter_from(0);
    }

    fn entry_matches(re: &Regex, e: &EntryRef) -> bool {
        re.is_match(e.message) || Self::other_columns_match(re, e)
    }

    /// Component or one of the extra columns of a log definition matches.
    fn other_columns_match(re: &Regex, e: &EntryRef) -> bool {
        re.is_match(e.component) || e.extras().any(|v| re.is_match(v))
    }

    /// (Re)loads the log definitions; a chosen definition is replaced by its new version.
    fn load_defs(&mut self) {
        let set = logdef::load_all();
        if let FormatChoice::Def(d) = &self.format_choice {
            self.format_choice = set
                .defs
                .iter()
                .find(|n| n.id.eq_ignore_ascii_case(&d.id))
                .cloned()
                .map_or(FormatChoice::Auto, FormatChoice::Def);
        }
        self.defs = Arc::new(set.defs);
        self.def_errors = set.errors;
        if !self.def_errors.is_empty() {
            self.status_error = Some(t!("error.logdef", count = self.def_errors.len()));
        }
    }

    /// Changes the format of the current file and reads it again.
    fn set_format(&mut self, choice: FormatChoice) {
        self.format_choice = choice;
        if self.cur_path.is_some() {
            self.reload_full();
        }
    }

    /// Header of a table column.
    fn column_title(c: &LogColumn) -> String {
        if let Some(t) = &c.title {
            return t.clone();
        }
        match c.kind {
            ColumnKind::Role(Role::Message) => t!("column.message"),
            ColumnKind::Role(Role::Component) => t!("column.component"),
            ColumnKind::Role(Role::DateTime) => t!("column.datetime"),
            ColumnKind::Role(Role::Thread) => t!("column.thread"),
            ColumnKind::Extra(_) => String::new(),
            ColumnKind::File => t!("column.file"),
        }
    }

    /// Text of a column for an entry (date/time formatted for display).
    fn cell_text(&self, e: &EntryRef, kind: ColumnKind) -> String {
        match kind {
            ColumnKind::Role(Role::Message) => e.message.to_owned(),
            ColumnKind::Role(Role::Component) => e.component.to_owned(),
            ColumnKind::Role(Role::DateTime) => self.display_dt(&e.datetime()),
            ColumnKind::Role(Role::Thread) => e.thread.to_owned(),
            ColumnKind::Extra(i) => e.extra(i).to_owned(),
            ColumnKind::File => self.file_label(e.file),
        }
    }

    fn sev_visible(&self, s: Severity) -> bool {
        match s {
            Severity::Info => self.show_sev[0],
            Severity::Warning => self.show_sev[1],
            Severity::Error => self.show_sev[2],
        }
    }

    /// Recomputes the filter from entry `start` on (earlier entries stay).
    fn refilter_from(&mut self, start: usize) {
        while self.filtered.last().is_some_and(|&i| i >= start) {
            self.filtered.pop();
        }
        self.is_match.truncate(start);
        for i in start..self.entries.len() {
            let e = self.entries.at(i);
            let m = self
                .search_re
                .as_ref()
                .is_some_and(|re| Self::entry_matches(re, &e));
            self.is_match.push(m);
            let pass_search = !self.filter_mode || self.search_re.is_none() || m;
            if pass_search
                && self.sev_visible(e.severity)
                && self.in_time_range(e.timestamp())
                && self.passes_column_filters(&e)
            {
                self.filtered.push(i);
            }
        }
        self.match_count = self.is_match.iter().filter(|&&b| b).count();
        if let Some(s) = self.selected
            && s >= self.entries.len()
        {
            self.selected = None;
        }
    }

    /// All column filters match the entry (the date/time as displayed).
    fn passes_column_filters(&self, e: &EntryRef) -> bool {
        self.col_filters.iter().all(|(kind, f)| match kind {
            ColumnKind::Role(Role::Message) => f.matches(e.message),
            ColumnKind::Role(Role::Component) => f.matches(e.component),
            ColumnKind::Role(Role::Thread) => f.matches(e.thread),
            ColumnKind::Role(Role::DateTime) => f.matches(&self.display_dt(&e.datetime())),
            ColumnKind::Extra(i) => f.matches(e.extra(*i)),
            ColumnKind::File => f.matches(self.file_labels.get(e.file as usize).map_or("", String::as_str)),
        })
    }

    /// The time range filter lets the entry through. With an active range, entries without
    /// a date/time are hidden (their time is unknown).
    fn in_time_range(&self, ts: Option<i64>) -> bool {
        match self.time_range {
            (None, None) => true,
            (from, to) => ts.is_some_and(|t| from.is_none_or(|f| t >= f) && to.is_none_or(|x| t <= x)),
        }
    }

    fn time_range_active(&self) -> bool {
        self.time_range != (None, None)
    }

    /// Parses the from / to texts and filters the table again.
    fn apply_time_range(&mut self) {
        let parse = |text: &str, end: bool| {
            if text.trim().is_empty() {
                (None, false)
            } else {
                let b = export::parse_range_bound(text, &self.date_format, end);
                (b, b.is_none())
            }
        };
        let (from, from_bad) = parse(&self.time_from, false);
        let (to, to_bad) = parse(&self.time_to, true);
        self.time_invalid = (from_bad, to_bad);
        if (from, to) != self.time_range {
            self.time_range = (from, to);
            self.filtered.clear();
            self.refilter_from(0);
        }
    }

    /// Protection of a long-watched log against filling the memory:
    /// 1. message text that grew past the temp file threshold moves to the temporary file,
    /// 2. above the size limit the oldest entries are removed (down to ¾ of the limit, so this
    ///    happens rarely and in one go).
    fn enforce_memory_limits(&mut self) {
        let mb = |v: u32| (v as usize) << 20;
        if self.cfg.temp_file_enabled
            && !self.entries.is_mapped()
            && self.entries.text_len() >= mb(self.cfg.temp_file_threshold_mb)
        {
            self.entries.move_text_to_temp_file();
        }
        if !self.cfg.max_log_enabled {
            return;
        }
        let limit = mb(self.cfg.max_log_mb.max(1));
        let len = self.entries.text_len();
        if len <= limit {
            return;
        }
        let k = self
            .entries
            .first_at_or_after(len - limit / 4 * 3)
            .min(self.committed_len);
        if k == 0 {
            return;
        }
        let pos_selected = self.selected.and_then(|s| self.filtered.binary_search(&s).ok());
        self.entries.drop_front(k);
        self.committed_len -= k;
        self.dropped += k;
        self.data_gen += 1;
        // Shift every entry index by k; indexes of removed entries disappear.
        let gone = self.filtered.partition_point(|&i| i < k);
        self.filtered.drain(..gone);
        for i in &mut self.filtered {
            *i -= k;
        }
        self.is_match.drain(..k.min(self.is_match.len()));
        self.match_count = self.is_match.iter().filter(|&&b| b).count();
        self.selected = self.selected.and_then(|s| s.checked_sub(k));
        self.code_cache = None;
        // Keep the selected row in view (the rows above it disappeared).
        if !self.cfg.auto_scroll
            && let Some(p) = pos_selected.and_then(|p| p.checked_sub(gone))
        {
            self.scroll_to = Some(p);
        }
    }

    /// Table columns: the columns of the log type, plus the File column first when the log is
    /// merged from several files (rotated parts).
    fn columns(&self) -> Vec<LogColumn> {
        let mut cols = self.schema.columns.clone();
        if self.files.len() > 1 {
            cols.insert(
                0,
                LogColumn {
                    kind: ColumnKind::File,
                    title: Some(t!("column.file")),
                },
            );
        }
        cols
    }

    fn set_files(&mut self, files: Vec<SourceFile>) {
        self.files = files;
        self.file_labels = (0..self.files.len() as u16).map(|i| self.file_label(i)).collect();
    }

    /// Name of a file of the log ("(overwritten)" when a later rotation replaced it).
    fn file_label(&self, id: u16) -> String {
        match self.files.get(id as usize) {
            Some(f) if f.path.is_some() => f.name.clone(),
            Some(f) => t!("file.overwritten", name = f.name),
            None => String::new(),
        }
    }

    /// Compiles the column filter texts and filters the table again.
    fn apply_column_filters(&mut self) {
        self.col_filters = self
            .columns()
            .iter()
            .filter_map(|c| {
                let text = self.col_filter_text.get(&c.kind)?;
                ColumnFilter::parse(text).map(|f| (c.kind, f))
            })
            .collect();
        self.filtered.clear();
        self.refilter_from(0);
    }

    /// Formats an ISO date-time ("2026-10-23 13:22:01.123") using the language pattern.
    fn display_dt(&self, iso: &str) -> String {
        let b = iso.as_bytes();
        if b.len() >= 10 && b[4] == b'-' && b[7] == b'-' {
            let date = self
                .date_format
                .replace("{y}", &iso[0..4])
                .replace("{m}", &iso[5..7])
                .replace("{d}", &iso[8..10]);
            format!("{date}{}", &iso[10..])
        } else {
            iso.to_owned()
        }
    }

    fn open_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter(t!("dialog.filter.logs"), &["log", "lo_", "txt"])
            .add_filter(t!("dialog.filter.all"), &["*"]);
        if let Some(dir) = self
            .cur_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        {
            dlg = dlg.set_directory(dir);
        } else if cfg!(windows) && std::path::Path::new(r"C:\Windows\CCM\Logs").exists() {
            dlg = dlg.set_directory(r"C:\Windows\CCM\Logs");
        }
        if let Some(p) = dlg.pick_file() {
            self.open_file(&p.to_string_lossy());
        }
    }

    fn open_file(&mut self, path: &str) {
        self.cancel_loading();
        self.data_gen += 1;
        self.cur_path = Some(PathBuf::from(path));
        self.source = Some(LogSource::new(path, self.cfg.keyword_severity));
        self.format_choice = FormatChoice::Auto;
        self.active_def = None;
        self.schema = Schema::default();
        self.set_files(Vec::new());
        self.dropped = 0;
        self.entries = LogStore::new();
        self.filtered.clear();
        self.is_match.clear();
        self.match_count = 0;
        self.committed_len = 0;
        self.selected = None;
        self.code_cache = None;
        self.last_refresh = None;
        self.last_refresh_wall = None;
        self.file_size = 0;
        self.status_error = None;
        self.pending_recent = Some(path.to_owned());
        self.start_poll();
    }

    /// Starts reading the file in a background thread (unless one is already running).
    fn start_poll(&mut self) {
        if self.loading.is_some() {
            return;
        }
        let Some(mut src) = self.source.take() else { return };
        src.keyword_severity = self.cfg.keyword_severity;
        src.defs = self.defs.clone();
        src.format = self.format_choice.clone();
        src.merge_rotated = self.cfg.merge_rotated;
        src.temp_file_threshold = self
            .cfg
            .temp_file_enabled
            .then(|| u64::from(self.cfg.temp_file_threshold_mb) << 20);
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let prog = progress.clone();
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("rustrace-reader".into())
            .spawn(move || {
                let res = src.poll(&prog);
                // If the UI no longer waits for the result (other file, cancelled), send fails – that's fine.
                let _ = tx.send((src, res));
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.loading = Some(Loading {
                    rx,
                    progress,
                    started: Instant::now(),
                });
            }
            Err(e) => {
                self.status_error = Some(t!("error.thread", error = e));
                self.recreate_source();
            }
        }
    }

    /// Takes over the result from the background thread when it is ready.
    fn check_loading(&mut self) {
        let Some(l) = &self.loading else { return };
        match l.rx.try_recv() {
            Ok((src, res)) => {
                self.loading = None;
                self.encoding_name = src.encoding_name();
                self.file_size = src.file_size;
                if self.schema != *src.schema() {
                    // Other columns (another log type): the column filters no longer apply.
                    if self.schema.columns != src.schema().columns {
                        self.col_filter_text.clear();
                        self.col_filters.clear();
                    }
                    self.schema = src.schema().clone();
                }
                self.active_def = src.active_def();
                if self.files != src.files() {
                    self.set_files(src.files().to_vec());
                }
                self.source = Some(src);
                self.apply_result(res);
                self.enforce_memory_limits();
                self.last_refresh = Some(Instant::now());
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                // The thread crashed – start over with the next refresh.
                self.loading = None;
                self.status_error = Some(t!("error.read_internal"));
                self.recreate_source();
                self.last_refresh = Some(Instant::now());
            }
        }
    }

    fn apply_result(&mut self, res: std::io::Result<PollResult>) {
        let before = self.filtered.len();
        // Provisional entries belong to the active file (the last one).
        let active = self.files.len().saturating_sub(1) as u16;
        if matches!(res, Ok(PollResult::Reset { .. }) | Ok(PollResult::Appended { .. })) {
            self.data_gen += 1;
        }
        match res {
            Ok(PollResult::Unchanged) => {
                self.status_error = None;
            }
            Ok(PollResult::Reset {
                committed,
                provisional,
            }) => {
                self.status_error = None;
                self.entries = committed;
                self.dropped = 0;
                self.committed_len = self.entries.len();
                for e in provisional {
                    self.entries.push(e, active);
                }
                self.selected = None;
                self.code_cache = None;
                self.filtered.clear();
                self.refilter_from(0);
                self.last_refresh_wall = Some(chrono::Local::now());
                if let Some(p) = self.pending_recent.take() {
                    self.cfg.push_recent(&p);
                    self.cfg_dirty = true;
                }
                if self.cfg.auto_scroll && !self.filtered.is_empty() {
                    self.scroll_to = Some(self.filtered.len() - 1);
                }
            }
            Ok(PollResult::Appended {
                committed,
                provisional,
            }) => {
                self.status_error = None;
                let start = self.committed_len;
                self.entries.truncate(start);
                self.entries.append(committed);
                self.committed_len = self.entries.len();
                for e in provisional {
                    self.entries.push(e, active);
                }
                if self.selected.is_some_and(|s| s >= start) {
                    self.code_cache = None;
                }
                self.refilter_from(start);
                self.last_refresh_wall = Some(chrono::Local::now());
                if self.cfg.auto_scroll
                    && self.filtered.len() != before
                    && !self.filtered.is_empty()
                {
                    self.scroll_to = Some(self.filtered.len() - 1);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => {
                self.status_error = Some(t!("error.read", error = e));
            }
        }
    }

    /// New source for the current file (the next read starts from the beginning).
    fn recreate_source(&mut self) {
        self.source = self
            .cur_path
            .as_ref()
            .map(|p| LogSource::new(p, self.cfg.keyword_severity));
    }

    /// Cancels loading in progress (the thread stops at the next chunk).
    fn cancel_loading(&mut self) {
        if let Some(l) = self.loading.take() {
            l.progress.cancel.store(true, Ordering::Relaxed);
            // The source stayed in the thread – create a new one, reading starts from the beginning.
            self.recreate_source();
        }
    }

    fn refresh(&mut self) {
        self.start_poll();
    }

    fn reload_full(&mut self) {
        self.cancel_loading();
        self.recreate_source();
        self.code_cache = None;
        self.start_poll();
    }

    fn find(&mut self, forward: bool) {
        let Some(re) = self.search_re.clone() else {
            self.focus_search = true;
            return;
        };
        let n = self.filtered.len();
        if n == 0 {
            return;
        }
        let cur = self
            .selected
            .and_then(|s| self.filtered.binary_search(&s).ok());
        for k in 1..=n {
            let pos = match cur {
                Some(p) if forward => (p + k) % n,
                Some(p) => (p + n - (k % n)) % n,
                None if forward => k - 1,
                None => n - k,
            };
            let idx = self.filtered[pos];
            if Self::entry_matches(&re, &self.entries.at(idx)) {
                self.select_row(pos);
                return;
            }
        }
        self.info(t!("search.not_found"));
    }

    /// Selects a row by its position in the displayed (filtered) list.
    fn select_row(&mut self, pos: usize) {
        if let Some(&idx) = self.filtered.get(pos) {
            self.selected = Some(idx);
            self.scroll_to = Some(pos);
            // Manual navigation stops following the end, otherwise the table would jump.
            if pos + 1 < self.filtered.len() {
                self.cfg.auto_scroll = false;
            }
        }
    }

    fn info(&mut self, s: String) {
        self.status_info = Some((s, Instant::now()));
    }

    fn tick_auto_refresh(&mut self, ctx: &egui::Context) {
        if self.cur_path.is_none() {
            return;
        }
        if self.loading.is_some() {
            // Repaint while loading to show progress.
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        if !self.cfg.auto_refresh {
            return;
        }
        let interval = Duration::from_secs_f64(self.cfg.interval_ms() / 1000.0);
        let due = self.last_refresh.is_none_or(|t| t.elapsed() >= interval);
        if due {
            self.start_poll();
            ctx.request_repaint_after(interval);
            return;
        }
        let elapsed = self.last_refresh.map(|t| t.elapsed()).unwrap_or_default();
        ctx.request_repaint_after(interval.saturating_sub(elapsed).max(Duration::from_millis(10)));
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        // Drag & drop of a file onto the window.
        let dropped = ctx
            .input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()))
            .filter(|p| !p.as_os_str().is_empty());
        if let Some(p) = dropped {
            self.open_file(&p.to_string_lossy());
        }

        let typing = ctx.egui_wants_keyboard_input();
        let export_key = ctx.input_mut(|i| {
            i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)
        });
        if export_key && self.cur_path.is_some() {
            self.show_export = true;
        }
        let (open, f5, ctrl_f5, ctrl_f, f3, sh_f3, pause, ctrl_h, ctrl_l, ctrl_e) =
            ctx.input_mut(|i| {
                (
                    i.consume_key(Modifiers::COMMAND, Key::O),
                    i.consume_key(Modifiers::NONE, Key::F5),
                    i.consume_key(Modifiers::COMMAND, Key::F5),
                    i.consume_key(Modifiers::COMMAND, Key::F),
                    i.consume_key(Modifiers::NONE, Key::F3),
                    i.consume_key(Modifiers::SHIFT, Key::F3),
                    i.consume_key(Modifiers::COMMAND, Key::P),
                    i.consume_key(Modifiers::COMMAND, Key::H),
                    i.consume_key(Modifiers::COMMAND, Key::L),
                    i.consume_key(Modifiers::COMMAND, Key::E),
                )
            });
        if open {
            self.open_dialog();
        }
        if ctrl_f5 {
            self.reload_full();
        } else if f5 {
            self.refresh();
        }
        if ctrl_f {
            self.focus_search = true;
        }
        if f3 {
            self.find(true);
        }
        if sh_f3 {
            self.find(false);
        }
        if pause {
            self.cfg.auto_refresh = !self.cfg.auto_refresh;
            self.cfg_dirty = true;
        }
        if ctrl_h {
            self.show_rules = true;
        }
        if ctrl_e {
            self.open_lookup(None);
        }
        if ctrl_l {
            self.filter_mode = !self.filter_mode;
            self.recompile_search();
        }

        if typing || self.filtered.is_empty() {
            return;
        }
        // Keyboard navigation in the table.
        let n = self.filtered.len();
        let cur = self
            .selected
            .and_then(|s| self.filtered.binary_search(&s).ok());
        let (up, down, pgup, pgdn, home, end, copy) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::PageUp),
                i.key_pressed(Key::PageDown),
                i.key_pressed(Key::Home),
                i.key_pressed(Key::End),
                i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
            )
        });
        let target = if up {
            Some(cur.map_or(n - 1, |p| p.saturating_sub(1)))
        } else if down {
            Some(cur.map_or(0, |p| (p + 1).min(n - 1)))
        } else if pgup {
            Some(cur.map_or(0, |p| p.saturating_sub(25)))
        } else if pgdn {
            Some(cur.map_or(0, |p| (p + 25).min(n - 1)))
        } else if home {
            Some(0)
        } else if end {
            self.cfg.auto_scroll = true;
            Some(n - 1)
        } else {
            None
        };
        if let Some(t) = target {
            self.select_row(t);
        }
        if copy && let Some(e) = self.selected.and_then(|i| self.entries.get(i)) {
            ctx.copy_text(e.message.to_owned());
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let t = match &self.cur_path {
            Some(p) => format!(
                "RusTrace – {}",
                p.file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
            None => "RusTrace".to_owned(),
        };
        if t != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(t.clone()));
            self.title = t;
        }
    }

    // ------------------------------------------------------------------
    // UI
    // ------------------------------------------------------------------

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(t!("menu.file"), |ui| {
                if ui.button(format!("📂 {}   {}", t!("menu.file.open"), kb("Ctrl+O"))).clicked() {
                    ui.close();
                    self.open_dialog();
                }
                ui.menu_button(t!("menu.file.recent"), |ui| {
                    if self.cfg.recent_files.is_empty() {
                        ui.weak(t!("menu.file.recent.none"));
                    }
                    let mut chosen = None;
                    for p in &self.cfg.recent_files {
                        if ui.button(p).clicked() {
                            chosen = Some(p.clone());
                        }
                    }
                    if let Some(p) = chosen {
                        ui.close();
                        self.open_file(&p);
                    }
                    if !self.cfg.recent_files.is_empty() {
                        ui.separator();
                        if ui.button(t!("menu.file.recent.clear")).clicked() {
                            self.cfg.recent_files.clear();
                            self.cfg_dirty = true;
                        }
                    }
                });
                ui.separator();
                if ui.button(format!("🔄 {}   F5", t!("menu.file.refresh"))).clicked() {
                    ui.close();
                    self.refresh();
                }
                if ui.button(format!("{}   {}", t!("menu.file.reload"), kb("Ctrl+F5"))).clicked() {
                    ui.close();
                    self.reload_full();
                }
                ui.separator();
                if ui
                    .add_enabled(
                        self.cur_path.is_some(),
                        egui::Button::new(format!(
                            "📤 {}   {}",
                            t!("menu.file.export"),
                            kb("Ctrl+Shift+S")
                        )),
                    )
                    .clicked()
                {
                    ui.close();
                    self.show_export = true;
                }
                ui.separator();
                if ui.button(t!("menu.file.exit")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(t!("menu.view"), |ui| {
                if ui
                    .checkbox(&mut self.cfg.show_details, t!("menu.view.details"))
                    .changed()
                {
                    self.cfg_dirty = true;
                }
                if ui
                    .checkbox(&mut self.cfg.auto_scroll, t!("menu.view.follow"))
                    .changed()
                {
                    self.cfg_dirty = true;
                }
                ui.separator();
                ui.label(t!("menu.view.theme"));
                let mut theme_changed = false;
                for (id, key) in [
                    ("system", "menu.view.theme.system"),
                    ("light", "menu.view.theme.light"),
                    ("dark", "menu.view.theme.dark"),
                ] {
                    if ui.radio(self.cfg.theme == id, t!(key)).clicked() {
                        self.cfg.theme = id.to_owned();
                        theme_changed = true;
                    }
                }
                if theme_changed {
                    self.apply_theme(ui.ctx());
                    self.cfg_dirty = true;
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(t!("menu.view.font_size"));
                    if ui
                        .add(
                            egui::Slider::new(&mut self.cfg.font_size, FONT_SIZE_RANGE)
                                .step_by(0.5)
                                .fixed_decimals(1),
                        )
                        .changed()
                    {
                        self.cfg_dirty = true;
                    }
                });
                ui.horizontal(|ui| {
                    let mut changed = ui
                        .checkbox(&mut self.cfg.temp_file_enabled, t!("menu.view.temp_file"))
                        .on_hover_text(t!("menu.view.temp_file.tip"))
                        .changed();
                    changed |= ui
                        .add_enabled(
                            self.cfg.temp_file_enabled,
                            egui::DragValue::new(&mut self.cfg.temp_file_threshold_mb)
                                .range(0..=100_000)
                                .suffix(" MB"),
                        )
                        .on_hover_text(t!("menu.view.temp_file.tip"))
                        .changed();
                    if changed {
                        self.cfg_dirty = true;
                    }
                });
                ui.horizontal(|ui| {
                    let mut changed = ui
                        .checkbox(&mut self.cfg.max_log_enabled, t!("menu.view.max_log"))
                        .on_hover_text(t!("menu.view.max_log.tip"))
                        .changed();
                    changed |= ui
                        .add_enabled(
                            self.cfg.max_log_enabled,
                            egui::DragValue::new(&mut self.cfg.max_log_mb)
                                .range(10..=1_000_000)
                                .suffix(" MB"),
                        )
                        .on_hover_text(t!("menu.view.max_log.tip"))
                        .changed();
                    if changed {
                        self.cfg_dirty = true;
                        self.enforce_memory_limits();
                    }
                });
                if ui
                    .checkbox(&mut self.cfg.merge_rotated, t!("menu.view.merge_rotated"))
                    .on_hover_text(t!("menu.view.merge_rotated.tip"))
                    .changed()
                {
                    self.cfg_dirty = true;
                    if self.cur_path.is_some() {
                        self.reload_full();
                    }
                }
                ui.separator();
                ui.menu_button(format!("📐 {}", t!("menu.view.format")), |ui| {
                    self.format_menu(ui);
                });
                ui.menu_button(format!("🌐 {}", t!("menu.view.language")), |ui| {
                    self.language_menu(ui);
                });
            });
            ui.menu_button(t!("menu.highlight"), |ui| {
                if ui
                    .button(format!("🎨 {}   {}", t!("menu.highlight.rules"), kb("Ctrl+H")))
                    .clicked()
                {
                    ui.close();
                    self.show_rules = true;
                }
                if ui.button(t!("menu.highlight.colors")).clicked() {
                    ui.close();
                    self.show_colors = true;
                }
            });
            ui.menu_button(t!("menu.tools"), |ui| {
                if ui
                    .button(format!("🔎 {}   {}", t!("menu.tools.lookup"), kb("Ctrl+E")))
                    .clicked()
                {
                    ui.close();
                    self.open_lookup(None);
                }
            });
            ui.menu_button(t!("menu.help"), |ui| {
                if ui.button(t!("menu.help.shortcuts")).clicked() {
                    ui.close();
                    self.show_help = true;
                }
                if let Some(p) = Config::path() {
                    ui.weak(t!("menu.help.settings_file", path = p.display()));
                }
            });
        });
    }

    /// Name of the format in use.
    /// Name of the format in use, with the file name of the log definition.
    fn format_name(&self) -> String {
        match &self.active_def {
            Some(d) => format!("{} – {}", d.name, d.id),
            None => t!("format.builtin"),
        }
    }

    /// Where a log definition comes from (user folder, folder next to the program, built-in).
    fn def_source(d: &LogDef) -> String {
        match &d.path {
            None => t!("format.source.builtin"),
            Some(p) if logdef::user_def_dir().is_some_and(|u| p.starts_with(u)) => {
                t!("format.source.user")
            }
            Some(_) => t!("format.source.exe"),
        }
    }

    /// Log definitions whose file name patterns match the open file (when chosen automatically
    /// and more than one matches – the first one is used).
    fn competing_defs(&self) -> Vec<Arc<LogDef>> {
        match (&self.cur_path, &self.format_choice) {
            (Some(p), FormatChoice::Auto) => {
                let m = logdef::matching_by_name(&self.defs, p);
                if m.len() > 1 { m } else { Vec::new() }
            }
            _ => Vec::new(),
        }
    }

    /// Tooltip of the format in the status bar: where the definition is loaded from.
    fn format_tip(&self) -> String {
        let mut tip = t!("status.format.tip");
        if let Some(d) = &self.active_def {
            tip.push('\n');
            tip.push_str(&d.path.as_ref().map_or_else(Self::builtin_label, |p| p.display().to_string()));
        }
        let competing = self.competing_defs();
        if !competing.is_empty() {
            tip.push_str("\n\n");
            tip.push_str(&t!("format.several"));
            for d in &competing {
                tip.push_str(&format!("\n• {} – {} ({})", d.name, d.id, Self::def_source(d)));
            }
        }
        tip
    }

    fn builtin_label() -> String {
        t!("format.source.builtin")
    }

    fn format_menu(&mut self, ui: &mut egui::Ui) {
        let mut choice: Option<FormatChoice> = None;
        let auto_label = if self.cur_path.is_some() {
            t!("format.auto_detected", name = self.format_name())
        } else {
            t!("format.auto")
        };
        if ui
            .radio(matches!(self.format_choice, FormatChoice::Auto), auto_label)
            .on_hover_text(t!("format.auto.tip"))
            .clicked()
        {
            choice = Some(FormatChoice::Auto);
        }
        if ui
            .radio(matches!(self.format_choice, FormatChoice::Builtin), t!("format.builtin"))
            .clicked()
        {
            choice = Some(FormatChoice::Builtin);
        }
        ui.separator();
        for d in self.defs.iter() {
            let selected = matches!(&self.format_choice, FormatChoice::Def(c) if c.id == d.id);
            let label = format!("{}  ({}, {})", d.name, d.id, Self::def_source(d));
            let mut r = ui.radio(selected, label);
            if let Some(p) = &d.path {
                r = r.on_hover_text(p.display().to_string());
            }
            if r.clicked() {
                choice = Some(FormatChoice::Def(d.clone()));
            }
        }
        // Several definitions match the open file: show which one is used and why.
        let competing = self.competing_defs();
        if !competing.is_empty() {
            ui.separator();
            ui.colored_label(ORANGE, format!("⚠ {}", t!("format.several")))
                .on_hover_text(t!("format.priority.tip"));
            for (i, d) in competing.iter().enumerate() {
                let mark = if i == 0 { "✔" } else { "    " };
                ui.label(format!("{mark} {} – {} ({})", d.name, d.id, Self::def_source(d)))
                    .on_hover_text(d.path.as_ref().map_or_else(Self::builtin_label, |p| p.display().to_string()));
            }
        }
        ui.separator();
        if ui.button(format!("🔄 {}", t!("format.reload"))).clicked() {
            self.load_defs();
            if self.def_errors.is_empty() {
                self.info(t!("format.reloaded", count = self.defs.len()));
            }
            if self.cur_path.is_some() {
                self.reload_full();
            }
            ui.close();
        }
        if ui.button(format!("📂 {}", t!("format.open_folder"))).clicked() {
            if let Some(dir) = logdef::user_def_dir() {
                let res = std::fs::create_dir_all(&dir).and_then(|_| open::that_detached(&dir));
                if let Err(e) = res {
                    self.status_error = Some(t!("error.open_folder", error = e));
                }
            }
            ui.close();
        }
        if let Some(dir) = logdef::user_def_dir() {
            ui.weak(dir.display().to_string());
        }
        if !self.def_errors.is_empty() {
            ui.separator();
            ui.colored_label(RED, t!("format.errors"));
            for e in &self.def_errors {
                ui.label(RichText::new(e).small().color(RED));
            }
        }
        if let Some(c) = choice {
            ui.close();
            self.set_format(c);
        }
    }

    fn language_menu(&mut self, ui: &mut egui::Ui) {
        let auto = self.cfg.language.is_empty();
        if ui
            .radio(auto, t!("lang.auto", code = i18n::system_language()))
            .clicked()
        {
            self.set_language("", false);
            ui.close();
        }
        ui.separator();
        for l in i18n::available() {
            let selected = !auto && self.lang_code.eq_ignore_ascii_case(&l.code);
            let mut label = format!("{}  ({})", l.name, l.code);
            if l.external {
                label.push_str(" *");
            }
            if ui.radio(selected, label).clicked() {
                self.set_language(&l.code, false);
                ui.close();
            }
        }
        ui.separator();
        if ui.button(format!("🔄 {}", t!("lang.reload"))).clicked() {
            let code = self.cfg.language.clone();
            self.set_language(&code, true);
            self.info(t!("lang.reloaded"));
            ui.close();
        }
        if ui.button(format!("📂 {}", t!("lang.open_folder"))).clicked() {
            if let Some(dir) = i18n::user_lang_dir() {
                let res = std::fs::create_dir_all(&dir).and_then(|_| open::that_detached(&dir));
                if let Err(e) = res {
                    self.status_error = Some(t!("error.open_folder", error = e));
                }
            }
            ui.close();
        }
        ui.menu_button(format!("➕ {}", t!("lang.new")), |ui| {
            ui.label(t!("lang.new.hint"));
            ui.horizontal(|ui| {
                ui.label(t!("lang.new.code"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_lang_code)
                        .hint_text("fr, pl, hu, pt-BR …")
                        .desired_width(90.0),
                );
            });
            let code = self.new_lang_code.trim().to_owned();
            let valid = !code.is_empty()
                && code.len() <= 12
                && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            if ui
                .add_enabled(valid, egui::Button::new(t!("lang.new.create")))
                .clicked()
            {
                match i18n::user_lang_dir()
                    .ok_or_else(|| std::io::Error::other("no config dir"))
                    .and_then(|dir| i18n::export_template(&dir, &code))
                {
                    Ok(path) => {
                        let _ = open::that_detached(&path);
                        self.info(t!("lang.new.created", path = path.display()));
                        self.new_lang_code.clear();
                        let lang = self.cfg.language.clone();
                        self.set_language(&lang, true);
                    }
                    Err(e) => self.status_error = Some(t!("error.create_lang", error = e)),
                }
                ui.close();
            }
        });
        if let Some(dir) = i18n::user_lang_dir() {
            ui.weak(dir.display().to_string());
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(format!("📂 {}", t!("toolbar.open")))
                .on_hover_text(kb("Ctrl+O"))
                .clicked()
            {
                self.open_dialog();
            }
            if ui
                .add_enabled(
                    self.cur_path.is_some(),
                    egui::Button::new(format!("🔄 {}", t!("toolbar.refresh"))),
                )
                .on_hover_text(t!("toolbar.refresh.tip"))
                .clicked()
            {
                self.refresh();
            }
            ui.separator();

            let (label, tip) = if self.cfg.auto_refresh {
                (format!("⏸ {}", t!("toolbar.pause")), t!("toolbar.pause.tip"))
            } else {
                (format!("▶ {}", t!("toolbar.resume")), t!("toolbar.resume.tip"))
            };
            let btn = egui::Button::new(label).selected(!self.cfg.auto_refresh);
            if ui.add(btn).on_hover_text(tip).clicked() {
                self.cfg.auto_refresh = !self.cfg.auto_refresh;
                self.cfg_dirty = true;
            }

            ui.label(t!("toolbar.interval"));
            let unit = self.cfg.refresh_unit;
            let speed = match unit {
                IntervalUnit::Ms => 10.0,
                IntervalUnit::Sec => 0.1,
                IntervalUnit::Min => 0.05,
            };
            if ui
                .add(
                    egui::DragValue::new(&mut self.cfg.refresh_value)
                        .range(unit.range())
                        .speed(speed)
                        .max_decimals(2),
                )
                .changed()
            {
                self.cfg_dirty = true;
            }
            let old_unit = self.cfg.refresh_unit;
            egui::ComboBox::from_id_salt("interval_unit")
                .selected_text(self.cfg.refresh_unit.label())
                .width(55.0)
                .show_ui(ui, |ui| {
                    for u in IntervalUnit::ALL {
                        ui.selectable_value(&mut self.cfg.refresh_unit, u, u.label());
                    }
                });
            if old_unit != self.cfg.refresh_unit {
                // Convert so the interval stays the same (e.g. 1500 ms → 1.5 s).
                let ms = old_unit.to_ms(self.cfg.refresh_value);
                let new = ms / self.cfg.refresh_unit.to_ms(1.0);
                let r = self.cfg.refresh_unit.range();
                self.cfg.refresh_value =
                    ((new * 100.0).round() / 100.0).clamp(*r.start(), *r.end());
                self.cfg_dirty = true;
            }
            ui.menu_button("⏱", |ui| {
                ui.label(t!("toolbar.presets"));
                for (v, u) in [
                    (100.0, IntervalUnit::Ms),
                    (250.0, IntervalUnit::Ms),
                    (500.0, IntervalUnit::Ms),
                    (1.0, IntervalUnit::Sec),
                    (2.0, IntervalUnit::Sec),
                    (5.0, IntervalUnit::Sec),
                    (10.0, IntervalUnit::Sec),
                    (30.0, IntervalUnit::Sec),
                    (1.0, IntervalUnit::Min),
                    (5.0, IntervalUnit::Min),
                ] {
                    if ui.button(format!("{v} {}", u.label())).clicked() {
                        self.cfg.refresh_value = v;
                        self.cfg.refresh_unit = u;
                        self.cfg.auto_refresh = true;
                        self.cfg_dirty = true;
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text(t!("toolbar.presets"));

            if ui
                .checkbox(&mut self.cfg.auto_scroll, t!("toolbar.follow"))
                .on_hover_text(t!("toolbar.follow.tip"))
                .changed()
            {
                self.cfg_dirty = true;
                if self.cfg.auto_scroll && !self.filtered.is_empty() {
                    self.scroll_to = Some(self.filtered.len() - 1);
                }
            }

            ui.separator();
            let mut sev_changed = false;
            sev_changed |= ui
                .toggle_value(&mut self.show_sev[0], format!("ℹ {}", t!("severity.info.plural")))
                .changed();
            sev_changed |= ui
                .toggle_value(&mut self.show_sev[1], format!("⚠ {}", t!("severity.warning.plural")))
                .changed();
            sev_changed |= ui
                .toggle_value(&mut self.show_sev[2], format!("⛔ {}", t!("severity.error.plural")))
                .changed();
            if sev_changed {
                self.filtered.clear();
                self.refilter_from(0);
            }
        });

        // Search row
        ui.horizontal_wrapped(|ui| {
            ui.label("🔍");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.search_text)
                    .hint_text(format!("{}  ({})", t!("search.hint"), kb("Ctrl+F")))
                    .desired_width(340.0)
                    .font(TextStyle::Monospace),
            );
            if self.focus_search {
                resp.request_focus();
                self.focus_search = false;
            }
            let mut changed = resp.changed();
            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let back = ui.input(|i| i.modifiers.shift);
                self.find(!back);
                resp.request_focus();
            }
            changed |= ui
                .checkbox(&mut self.search_regex, "Regex")
                .on_hover_text(t!("search.regex.tip"))
                .changed();
            changed |= ui
                .checkbox(&mut self.search_case, "Aa")
                .on_hover_text(t!("search.case.tip"))
                .changed();
            changed |= ui
                .checkbox(&mut self.filter_mode, t!("search.filter"))
                .on_hover_text(t!("search.filter.tip"))
                .changed();
            if changed {
                self.recompile_search();
            }
            let active = self.col_filters.len();
            let label = if active > 0 {
                t!("toolbar.column_filters.active", count = active)
            } else {
                t!("toolbar.column_filters")
            };
            if ui
                .toggle_value(&mut self.cfg.show_column_filters, label)
                .on_hover_text(t!("toolbar.column_filters.tip"))
                .changed()
            {
                self.cfg_dirty = true;
            }
            let label = if self.time_range_active() {
                t!("toolbar.time_range.active")
            } else {
                t!("toolbar.time_range")
            };
            if ui
                .toggle_value(&mut self.cfg.show_time_range, format!("🕑 {label}"))
                .on_hover_text(t!("toolbar.time_range.tip"))
                .changed()
            {
                self.cfg_dirty = true;
            }
            if ui.button("⬆").on_hover_text(t!("search.prev.tip")).clicked() {
                self.find(false);
            }
            if ui.button("⬇").on_hover_text(t!("search.next.tip")).clicked() {
                self.find(true);
            }
            if let Some(err) = &self.search_err {
                ui.colored_label(RED, format!("⚠ {}", t!("search.invalid_regex")))
                    .on_hover_text(err);
            } else if self.search_re.is_some() {
                ui.label(t!("search.matches", count = self.match_count));
            }
            if !self.search_text.is_empty()
                && ui
                    .small_button("✖")
                    .on_hover_text(t!("search.clear.tip"))
                    .clicked()
            {
                self.search_text.clear();
                self.recompile_search();
            }
            ui.separator();
            let (icon, name) = match self.cfg.theme.as_str() {
                "light" => ("☀", t!("menu.view.theme.light")),
                "dark" => ("🌙", t!("menu.view.theme.dark")),
                _ => ("🖥", t!("menu.view.theme.system")),
            };
            if ui
                .button(icon)
                .on_hover_text(t!("toolbar.theme.tip", theme = name))
                .clicked()
            {
                let ctx = ui.ctx().clone();
                self.cycle_theme(&ctx);
            }
            if ui
                .button(format!("🎨 {}", t!("toolbar.highlighting")))
                .on_hover_text(kb("Ctrl+H"))
                .clicked()
            {
                self.show_rules = true;
            }
        });
        if self.cfg.show_time_range {
            self.time_range_row(ui);
        }
        ui.add_space(2.0);
    }

    /// Time range filter: from / to fields with presets.
    fn time_range_row(&mut self, ui: &mut egui::Ui) {
        let latest = self.latest_ts();
        let example = self.input_dt(latest.as_deref().unwrap_or("2026-10-23 08:00:00.000"));
        let selected = self
            .selected
            .and_then(|i| self.entries.get(i))
            .and_then(|e| e.timestamp())
            .map(|ms| self.input_dt(&ms_to_iso(ms)));
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("🕑 {}", t!("time.from")));
            fn field<'a>(text: &'a mut String, hint: &str) -> egui::TextEdit<'a> {
                egui::TextEdit::singleline(text)
                    .hint_text(hint)
                    .desired_width(170.0)
                    .font(TextStyle::Monospace)
            }
            changed |= ui
                .add(field(&mut self.time_from, &example))
                .on_hover_text(t!("time.tip"))
                .changed();
            if self.time_invalid.0 {
                ui.colored_label(RED, "⚠").on_hover_text(t!("time.invalid", example = example));
            }
            ui.label(t!("time.to"));
            changed |= ui
                .add(field(&mut self.time_to, &example))
                .on_hover_text(t!("time.tip"))
                .changed();
            if self.time_invalid.1 {
                ui.colored_label(RED, "⚠").on_hover_text(t!("time.invalid", example = example));
            }
            ui.separator();
            if ui
                .add_enabled(selected.is_some(), egui::Button::new(t!("time.from_selected")))
                .clicked()
            {
                self.time_from = selected.clone().unwrap_or_default();
                changed = true;
            }
            if ui
                .add_enabled(selected.is_some(), egui::Button::new(t!("time.to_selected")))
                .clicked()
            {
                self.time_to = selected.clone().unwrap_or_default();
                changed = true;
            }
            for (hours, key) in [(1, "export.since.last_hour"), (24, "export.since.last_day")] {
                if ui
                    .add_enabled(latest.is_some(), egui::Button::new(t!(key)))
                    .on_hover_text(t!("export.since.presets.tip"))
                    .clicked()
                {
                    let from = latest.as_deref().and_then(|l| export::hours_before(l, hours));
                    self.time_from = from.map(|f| self.input_dt(&f)).unwrap_or_default();
                    self.time_to.clear();
                    changed = true;
                }
            }
            if ui
                .add_enabled(
                    !self.time_from.is_empty() || !self.time_to.is_empty(),
                    egui::Button::new(format!("✖ {}", t!("time.clear"))),
                )
                .clicked()
            {
                self.time_from.clear();
                self.time_to.clear();
                changed = true;
            }
            if self.cur_path.is_some() && latest.is_none() && !self.entries.is_empty() {
                ui.colored_label(ORANGE, t!("time.no_time"));
            }
        });
        if changed {
            self.apply_time_range();
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(path) = &self.cur_path {
                ui.label(path.display().to_string());
                ui.separator();
                if !self.encoding_name.is_empty() {
                    ui.label(self.encoding_name);
                    ui.separator();
                }
                let warn = if self.competing_defs().is_empty() { "" } else { " ⚠" };
                ui.label(format!("📐 {}{warn}", self.format_name()))
                    .on_hover_text(self.format_tip());
                ui.separator();
                if self.file_size > 0 {
                    ui.label(format_size(self.file_size));
                    ui.separator();
                }
                if self.dropped > 0 {
                    ui.colored_label(ORANGE, format!("✂ {}", t!("status.dropped", count = self.dropped)))
                        .on_hover_text(t!("status.dropped.tip", limit = self.cfg.max_log_mb));
                    ui.separator();
                }
                if self.files.len() > 1 {
                    ui.label(format!("📚 {}", t!("status.parts", count = self.files.len())))
                        .on_hover_text(self.file_labels.join("\n"));
                    ui.separator();
                }
                if self.entries.is_mapped() {
                    ui.label("💾").on_hover_text(t!(
                        "status.temp_file",
                        size = format_size(self.entries.text_len() as u64)
                    ));
                    ui.separator();
                }
                if self.filtered.len() == self.entries.len() {
                    ui.label(t!("status.entries", count = self.entries.len()));
                } else {
                    ui.label(t!(
                        "status.shown",
                        shown = self.filtered.len(),
                        total = self.entries.len()
                    ));
                }
                ui.separator();
                if self.cfg.auto_refresh {
                    ui.label(format!(
                        "🔄 {}",
                        t!(
                            "status.refresh_every",
                            value = trim_num(self.cfg.refresh_value),
                            unit = self.cfg.refresh_unit.label()
                        )
                    ));
                } else {
                    ui.colored_label(ORANGE, format!("⏸ {}", t!("status.paused")));
                }
                if let Some(time) = &self.last_refresh_wall {
                    ui.separator();
                    ui.label(t!("status.last_change", time = time.format("%H:%M:%S")));
                }
            } else {
                ui.label(t!("status.no_file"));
            }
            // During the first load the progress is in the middle of the window; here only for later refreshes.
            if self.loading.is_some() && !self.entries.is_empty() {
                ui.separator();
                self.progress_ui(ui, false);
            }
            if let Some(e) = &self.status_error {
                ui.separator();
                ui.colored_label(RED, e);
            }
            if let Some((msg, time)) = &self.status_info
                && time.elapsed() < Duration::from_secs(3)
            {
                ui.separator();
                ui.colored_label(ORANGE, msg);
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        });
    }

    /// Error codes in the selected row (cached).
    fn selected_codes(&mut self) -> Vec<CodeInfo> {
        let Some(i) = self.selected else { return Vec::new() };
        if let Some((ci, c)) = &self.code_cache
            && *ci == i
        {
            return c.clone();
        }
        let codes: Vec<CodeInfo> = self
            .entries
            .get(i)
            .map(|e| errcodes::find_codes(e.message))
            .unwrap_or_default()
            .into_iter()
            .map(errcodes::lookup)
            .collect();
        self.code_cache = Some((i, codes.clone()));
        codes
    }

    fn open_lookup(&mut self, code: Option<u32>) {
        if let Some(v) = code {
            self.lookup_input = if v < 0x1_0000 {
                v.to_string()
            } else {
                format!("0x{v:08X}")
            };
        }
        self.show_lookup = true;
        self.lookup_focus = true;
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        let codes = self.selected_codes();
        let mut open_code: Option<u32> = None;
        let Some(e) = self.selected.and_then(|i| self.entries.get(i)) else {
            ui.centered_and_justified(|ui| {
                ui.weak(t!("details.empty"));
            });
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong(t!("details.line", line = e.line_no));
            ui.separator();
            let (txt, col) = match e.severity {
                Severity::Error => (t!("severity.error"), c32(self.cfg.palette(self.dark).error.1)),
                Severity::Warning => (t!("severity.warning"), Color32::from_rgb(200, 160, 0)),
                Severity::Info => (t!("severity.info"), ui.visuals().text_color()),
            };
            ui.colored_label(col, txt);
            // Every column except the message (shown below), in the order of the table.
            for c in &self.columns() {
                let value = match c.kind {
                    ColumnKind::Role(Role::Message) => continue,
                    kind => self.cell_text(&e, kind),
                };
                if value.is_empty() {
                    continue;
                }
                ui.separator();
                let label = match (c.kind, &c.title) {
                    (_, Some(title)) => format!("{title}: {value}"),
                    (ColumnKind::Role(Role::Component), None) => {
                        t!("details.component", component = value)
                    }
                    (ColumnKind::Role(Role::DateTime), None) => t!("details.time", time = value),
                    (ColumnKind::Role(Role::Thread), None) => t!("details.thread", thread = value),
                    _ => value,
                };
                ui.label(label);
            }
        });
        // Explanation of error codes found in the message.
        for c in &codes {
            ui.horizontal(|ui| {
                if ui
                    .small_button(RichText::new(format!("🔎 {}", c.hex())).monospace())
                    .on_hover_text(t!("details.code.open_tip"))
                    .clicked()
                {
                    open_code = Some(c.hresult);
                }
                let col = if c.descriptions.is_empty() {
                    ui.visuals().weak_text_color()
                } else {
                    ui.visuals().strong_text_color()
                };
                let mut tip = format!("{}\n{}", c.forms(), c.facility);
                for (src, d) in &c.descriptions {
                    tip.push_str(&format!("\n\n{src}: {d}"));
                }
                ui.add(egui::Label::new(RichText::new(c.short()).color(col)).truncate())
                    .on_hover_text(tip);
            });
        }
        ui.separator();
        let font = FontId::monospace(self.cfg.font_size);
        let out = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut s = e.message;
                ui.add(
                    egui::TextEdit::multiline(&mut s)
                        .desired_width(f32::INFINITY)
                        .font(font),
                );
            });
        self.hover_log |= ui.rect_contains_pointer(out.inner_rect);
        if open_code.is_some() {
            self.open_lookup(open_code);
        }
    }

    /// Whole-row colors: (foreground, background).
    fn row_colors(&self, e: &EntryRef) -> (Option<Color32>, Option<Color32>) {
        // "Whole row" rules take precedence – the first matching one wins.
        for r in &self.rules {
            if r.rule.scope != HighlightScope::Row {
                continue;
            }
            let hit = r.regex.is_match(e.message)
                || (r.rule.match_component && Self::other_columns_match(&r.regex, e));
            if hit {
                let (fg, bg) = r.rule.colors(self.dark);
                return (r.rule.use_fg.then(|| c32(fg)), r.rule.use_bg.then(|| c32(bg)));
            }
        }
        if self.cfg.color_by_severity {
            let p = self.cfg.palette(self.dark);
            let pair = match e.severity {
                Severity::Error => Some(p.error),
                Severity::Warning => Some(p.warning),
                Severity::Info => None,
            };
            if let Some((fg, bg)) = pair {
                return (Some(c32(fg)), Some(c32(bg)));
            }
        }
        (None, None)
    }

    fn text_spans(&self, text: &str, search_hit: bool) -> Vec<Span> {
        let mut spans = Vec::new();
        // Added from the last rule – later spans override earlier ones,
        // so the first rule in the list has the highest priority.
        for r in self.rules.iter().rev() {
            if r.rule.scope != HighlightScope::Text {
                continue;
            }
            let (fg, bg) = r.rule.colors(self.dark);
            for m in r.regex.find_iter(text).take(200) {
                if m.start() < m.end() {
                    spans.push(Span {
                        start: m.start(),
                        end: m.end(),
                        fg: r.rule.use_fg.then(|| c32(fg)),
                        bg: r.rule.use_bg.then(|| c32(bg)),
                    });
                }
            }
        }
        // Search matches are on top.
        if search_hit && let Some(re) = &self.search_re {
            let (sfg, sbg) = self.cfg.palette(self.dark).search;
            for m in re.find_iter(text).take(200) {
                if m.start() < m.end() {
                    spans.push(Span {
                        start: m.start(),
                        end: m.end(),
                        fg: Some(c32(sfg)),
                        bg: Some(c32(sbg)),
                    });
                }
            }
        }
        spans
    }

    fn table(&mut self, ui: &mut egui::Ui) {
        self.hover_log |= ui.rect_contains_pointer(ui.max_rect());
        if self.loading.is_some() && self.entries.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.35);
                ui.set_max_width(480.0);
                self.progress_ui(ui, true);
            });
            return;
        }
        if self.cur_path.is_none() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(t!("table.empty")).size(18.0).weak());
            });
            return;
        }

        let font = FontId::proportional(self.cfg.font_size);
        let row_h = (self.cfg.font_size * 1.45).round() + 2.0;
        let default_fg = ui.visuals().text_color();
        let gap = egui::vec2(
            ui.spacing().item_spacing.x,
            ui.spacing().item_spacing.y * 0.5 + 0.5,
        );

        // Horizontal scrolling (the table itself only scrolls vertically): the table is laid out
        // shifted left by the offset and clipped to the visible area, so its right edge with the
        // vertical scroll bar always stays in view. A horizontal scroll bar below it appears when
        // the columns do not fit (last frame's width); Shift + mouse wheel scrolls as well.
        let parent = ui;
        let full = parent.available_rect_before_wrap();
        let max_offset = (self.table_width - full.width()).max(0.0);
        let hbar_h = if max_offset > 0.0 {
            parent.spacing().scroll.bar_width + 4.0
        } else {
            0.0
        };
        let view = egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, full.max.y - hbar_h));
        if parent.rect_contains_pointer(view) {
            self.table_scroll_x -= parent.input(|i| i.smooth_scroll_delta.x);
        }
        self.table_scroll_x = self.table_scroll_x.clamp(0.0, max_offset);
        let mut child = parent.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
            egui::pos2(view.min.x - self.table_scroll_x, view.min.y),
            view.max,
        )));
        child.set_clip_rect(view.intersect(parent.clip_rect()));
        let ui = &mut child;

        // The columns come from the log format; every layout keeps its own column widths.
        let columns = self.columns();
        let mut tb = TableBuilder::new(ui)
            .id_salt(("log_table", &self.schema.name, columns.len()))
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .stick_to_bottom(self.cfg.auto_scroll)
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::initial(60.0).at_least(40.0).clip(true));
        // With many columns (e.g. IIS) a narrower message leaves room for the others.
        let msg_width = if columns.len() > 5 { 420.0 } else { 720.0 };
        for (i, c) in columns.iter().enumerate() {
            let col = if i + 1 == columns.len() {
                Column::remainder().at_least(LAST_COLUMN_MIN)
            } else {
                match c.kind {
                    ColumnKind::Role(Role::Message) => Column::initial(msg_width).at_least(120.0),
                    ColumnKind::Role(Role::DateTime) => Column::initial(175.0).at_least(40.0),
                    _ => Column::initial(150.0).at_least(40.0),
                }
            };
            tb = tb.column(col.clip(true));
        }
        if let Some(r) = self.scroll_to.take() {
            tb = tb.scroll_to_row(r, Some(Align::Center));
        }

        let mut clicked: Option<usize> = None;
        let mut action: Option<RowAction> = None;
        let headers: Vec<String> = std::iter::once(t!("column.line"))
            .chain(columns.iter().map(Self::column_title))
            .collect();

        // Filter row under the headers: one field per column of the current log type.
        let show_filters = self.cfg.show_column_filters;
        let mut filter_texts: Vec<String> = columns
            .iter()
            .map(|c| self.col_filter_text.get(&c.kind).cloned().unwrap_or_default())
            .collect();
        let mut filters_changed = false;
        let mut clear_filters = false;
        let any_filter = !self.col_filters.is_empty();
        let header_h = if show_filters { 48.0 } else { 22.0 };
        // Left edges of the first and the last header cell → natural width of the table.
        let last_col = headers.len() - 1;
        let mut edges = (0.0f32, 0.0f32);

        tb.header(header_h, |mut h| {
            for (i, title) in headers.into_iter().enumerate() {
                let (cell, _) = h.col(|ui| {
                    ui.vertical(|ui| {
                        ui.strong(title);
                        if !show_filters {
                            return;
                        }
                        if i == 0 {
                            // The line column: button that clears all column filters.
                            if ui
                                .add_enabled(any_filter, egui::Button::new("✖").small())
                                .on_hover_text(t!("filter.clear"))
                                .clicked()
                            {
                                clear_filters = true;
                            }
                        } else {
                            let text = &mut filter_texts[i - 1];
                            let active = ColumnFilter::parse(text).is_some();
                            let mut edit = egui::TextEdit::singleline(text)
                                .id_salt(("column_filter", i))
                                .hint_text(t!("filter.hint"))
                                .desired_width(f32::INFINITY);
                            if active {
                                edit = edit.background_color(c32(self.cfg.palette(self.dark).search.1).gamma_multiply(0.35));
                            }
                            filters_changed |=
                                ui.add(edit).on_hover_text(t!("filter.tip")).changed();
                        }
                    });
                });
                if i == 0 {
                    edges.0 = cell.left();
                }
                if i == last_col {
                    edges.1 = cell.left();
                }
            }
        })
        .body(|body| {
            body.rows(row_h, self.filtered.len(), |mut row| {
                let idx = self.filtered[row.index()];
                let e = self.entries.at(idx);
                let is_sel = self.selected == Some(idx);
                row.set_selected(is_sel);

                let (fg, bg) = if is_sel {
                    (None, None)
                } else {
                    self.row_colors(&e)
                };
                let fg_col = fg.unwrap_or(default_fg);

                let paint_bg = |ui: &mut egui::Ui| {
                    if let Some(bg) = bg {
                        let r = ui.max_rect().expand2(gap);
                        ui.painter().rect_filled(r, 0.0, bg);
                    }
                };
                let simple = |ui: &mut egui::Ui, s: &str| {
                    paint_bg(ui);
                    ui.add(
                        egui::Label::new(RichText::new(s).font(font.clone()).color(fg_col))
                            .truncate()
                            .selectable(false),
                    );
                };

                row.col(|ui| simple(ui, &e.line_no.to_string()));
                for c in &columns {
                    match c.kind {
                        ColumnKind::Role(Role::Message) => {
                            row.col(|ui| {
                                paint_bg(ui);
                                let first = e.message.lines().next().unwrap_or("");
                                let mut text: String = if first.len() > MAX_CELL_CHARS {
                                    let mut end = MAX_CELL_CHARS;
                                    while !first.is_char_boundary(end) {
                                        end -= 1;
                                    }
                                    format!("{}…", &first[..end])
                                } else {
                                    first.to_owned()
                                };
                                if e.message.contains('\n') {
                                    text.push_str("  ↵");
                                }
                                let hit = self.is_match.get(idx).copied().unwrap_or(false);
                                let spans = self.text_spans(&text, hit);
                                let job = build_job(&text, &spans, fg_col, &font);
                                ui.add(egui::Label::new(job).truncate().selectable(false));
                            });
                        }
                        ColumnKind::Role(Role::Component) => {
                            row.col(|ui| simple(ui, e.component));
                        }
                        ColumnKind::Role(Role::Thread) => {
                            row.col(|ui| simple(ui, e.thread));
                        }
                        kind => {
                            row.col(|ui| simple(ui, &self.cell_text(&e, kind)));
                        }
                    }
                }

                let resp = row.response();
                if resp.clicked() {
                    clicked = Some(idx);
                }
                resp.context_menu(|ui| {
                    if ui.button(t!("context.copy_message")).clicked() {
                        action = Some(RowAction::CopyMessage(idx));
                        ui.close();
                    }
                    if ui.button(t!("context.copy_row")).clicked() {
                        action = Some(RowAction::CopyRow(idx));
                        ui.close();
                    }
                    let codes = errcodes::find_codes(e.message);
                    if !codes.is_empty() {
                        ui.separator();
                        for v in codes {
                            let code = if v < 0x1_0000 {
                                v.to_string()
                            } else {
                                format!("0x{v:08X}")
                            };
                            if ui
                                .button(format!("🔎 {}", t!("context.explain_code", code = code)))
                                .clicked()
                            {
                                action = Some(RowAction::ExplainCode(v));
                                ui.close();
                            }
                        }
                    }
                    if !e.component.is_empty() {
                        ui.separator();
                        if ui
                            .button(t!("context.search_component", component = e.component))
                            .clicked()
                        {
                            action = Some(RowAction::SearchComponent(idx));
                            ui.close();
                        }
                        if ui
                            .button(t!("context.highlight_component", component = e.component))
                            .clicked()
                        {
                            action = Some(RowAction::RuleFromComponent(idx));
                            ui.close();
                        }
                    }
                });
            });
        });

        // Natural width: all columns up to the last one plus its minimum width (the last column
        // fills the rest when everything fits).
        self.table_width = edges.1 - edges.0 + LAST_COLUMN_MIN;
        parent.advance_cursor_after_rect(view);
        if hbar_h > 0.0 {
            let bar = egui::Rect::from_min_max(egui::pos2(full.min.x, view.max.y), full.max);
            let mut bar_ui = parent.new_child(egui::UiBuilder::new().max_rect(bar));
            bar_ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            let out = egui::ScrollArea::horizontal()
                .id_salt("log_table_hscroll")
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .horizontal_scroll_offset(self.table_scroll_x)
                .show(&mut bar_ui, |ui| {
                    ui.allocate_space(egui::vec2(self.table_width, 1.0));
                });
            self.table_scroll_x = out.state.clamped_offset().x;
        }

        if let Some(i) = clicked {
            self.selected = Some(i);
        }
        if clear_filters {
            self.col_filter_text.clear();
            self.apply_column_filters();
        } else if filters_changed {
            for (c, text) in columns.iter().zip(filter_texts) {
                if text.is_empty() {
                    self.col_filter_text.remove(&c.kind);
                } else {
                    self.col_filter_text.insert(c.kind, text);
                }
            }
            self.apply_column_filters();
        }
        if let Some(a) = action {
            let ctx = ui.ctx().clone();
            match a {
                RowAction::ExplainCode(v) => self.open_lookup(Some(v)),
                RowAction::CopyMessage(i) => ctx.copy_text(self.entries.at(i).message.to_owned()),
                RowAction::CopyRow(i) => {
                    let e = self.entries.at(i);
                    let cells: Vec<String> =
                        columns.iter().map(|c| self.cell_text(&e, c.kind)).collect();
                    ctx.copy_text(cells.join("\t"));
                }
                RowAction::SearchComponent(i) => {
                    self.search_text = self.entries.at(i).component.to_owned();
                    self.search_regex = false;
                    self.recompile_search();
                }
                RowAction::RuleFromComponent(i) => {
                    let comp = self.entries.at(i).component.to_owned();
                    self.cfg.rules.insert(
                        0,
                        HighlightRule {
                            name: t!("rule.component_name", component = comp),
                            pattern: format!("^{}$", regex::escape(&comp)),
                            is_regex: true,
                            match_component: true,
                            ..Default::default()
                        },
                    );
                    self.recompile_rules();
                    self.cfg_dirty = true;
                    self.show_rules = true;
                }
            }
        }
    }

    fn rules_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_rules;
        let mut changed = false;
        egui::Window::new(format!("🎨 {}", t!("rules.title")))
            .id(egui::Id::unique("rules_window"))
            .open(&mut open)
            .default_width(1150.0)
            .min_width(900.0)
            .default_height(420.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(t!("rules.help"));
                ui.separator();
                let edit_dark = self.edit_dark;
                ui.horizontal(|ui| {
                    theme_selector(ui, &mut self.edit_dark);
                    let other = theme_name(!edit_dark);
                    if ui
                        .button(format!("⇄ {}", t!("colors.copy_from", theme = other)))
                        .on_hover_text(t!("colors.copy_from.tip"))
                        .clicked()
                    {
                        for r in &mut self.cfg.rules {
                            r.copy_colors(!edit_dark);
                        }
                        changed = true;
                    }
                });
                ui.separator();
                let mut act: Option<(usize, i32)> = None; // (index, -1 up / 1 down / 0 delete)
                egui::ScrollArea::both()
                    .max_height(ui.available_height() - 40.0)
                    .show(ui, |ui| {
                        egui::Grid::new("rules_grid")
                            .striped(true)
                            .spacing([8.0, 6.0])
                            .show(ui, |ui| {
                                for h in [
                                    t!("rules.col.on"),
                                    t!("rules.col.name"),
                                    t!("rules.col.pattern"),
                                    String::new(),
                                    "Regex".to_owned(),
                                    "Aa".to_owned(),
                                    t!("rules.col.scope"),
                                    t!("rules.col.component"),
                                    t!("rules.col.text"),
                                    t!("rules.col.background"),
                                    t!("rules.col.preview"),
                                    String::new(),
                                ] {
                                    ui.strong(h);
                                }
                                ui.end_row();

                                let n = self.cfg.rules.len();
                                for i in 0..n {
                                    let err = self.rule_errors.get(i).cloned().flatten();
                                    let r = &mut self.cfg.rules[i];
                                    ui.push_id(i, |ui| {
                                        changed |= ui.checkbox(&mut r.enabled, "").changed();
                                    });
                                    changed |= ui
                                        .add_sized(
                                            [150.0, 20.0],
                                            egui::TextEdit::singleline(&mut r.name),
                                        )
                                        .changed();
                                    changed |= ui
                                        .add_sized(
                                            [300.0, 20.0],
                                            egui::TextEdit::singleline(&mut r.pattern)
                                                .font(TextStyle::Monospace)
                                                .hint_text(t!("rules.pattern.hint")),
                                        )
                                        .changed();
                                    if let Some(e) = err {
                                        ui.colored_label(RED, "⚠").on_hover_text(e);
                                    } else {
                                        ui.label("");
                                    }
                                    changed |= ui.checkbox(&mut r.is_regex, "").changed();
                                    changed |= ui
                                        .checkbox(&mut r.case_sensitive, "")
                                        .on_hover_text(t!("search.case.tip"))
                                        .changed();
                                    let before = r.scope;
                                    egui::ComboBox::from_id_salt(("scope", i))
                                        .selected_text(scope_label(r.scope))
                                        .width(110.0)
                                        .show_ui(ui, |ui| {
                                            for s in [HighlightScope::Row, HighlightScope::Text] {
                                                ui.selectable_value(&mut r.scope, s, scope_label(s));
                                            }
                                        });
                                    changed |= before != r.scope;
                                    let comp_enabled = r.scope == HighlightScope::Row;
                                    changed |= ui
                                        .add_enabled(
                                            comp_enabled,
                                            egui::Checkbox::new(&mut r.match_component, ""),
                                        )
                                        .on_hover_text(t!("rules.component.tip"))
                                        .changed();
                                    ui.horizontal(|ui| {
                                        changed |= ui.checkbox(&mut r.use_fg, "").changed();
                                        let (fg, _) = r.colors_mut(edit_dark);
                                        changed |= color_button(ui, fg);
                                    });
                                    ui.horizontal(|ui| {
                                        changed |= ui.checkbox(&mut r.use_bg, "").changed();
                                        let (_, bg) = r.colors_mut(edit_dark);
                                        changed |= color_button(ui, bg);
                                    });
                                    let (fg, bg) = r.colors(edit_dark);
                                    preview(
                                        ui,
                                        edit_dark,
                                        r.use_fg.then_some(fg),
                                        r.use_bg.then_some(bg),
                                    );
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add_enabled(i > 0, egui::Button::new("⬆").small())
                                            .clicked()
                                        {
                                            act = Some((i, -1));
                                        }
                                        if ui
                                            .add_enabled(i + 1 < n, egui::Button::new("⬇").small())
                                            .clicked()
                                        {
                                            act = Some((i, 1));
                                        }
                                        if ui
                                            .small_button("🗑")
                                            .on_hover_text(t!("rules.delete"))
                                            .clicked()
                                        {
                                            act = Some((i, 0));
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                    });
                if let Some((i, d)) = act {
                    match d {
                        -1 => self.cfg.rules.swap(i, i - 1),
                        1 => self.cfg.rules.swap(i, i + 1),
                        _ => {
                            self.cfg.rules.remove(i);
                        }
                    }
                    changed = true;
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(format!("➕ {}", t!("rules.add"))).clicked() {
                        self.cfg.rules.push(HighlightRule::default());
                        changed = true;
                    }
                    if ui.button(format!("📥 {}", t!("rules.import"))).clicked()
                        && let Some(p) = rfd::FileDialog::new()
                            .add_filter("JSON", &["json"])
                            .pick_file()
                    {
                        match std::fs::read_to_string(&p)
                            .map_err(|e| e.to_string())
                            .and_then(|s| {
                                serde_json::from_str::<Vec<HighlightRule>>(&s)
                                    .map_err(|e| e.to_string())
                            }) {
                            Ok(rules) => {
                                self.cfg.rules.extend(rules);
                                self.cfg.fill_missing_dark();
                                changed = true;
                            }
                            Err(e) => self.status_error = Some(t!("error.import", error = e)),
                        }
                    }
                    if ui.button(format!("📤 {}", t!("rules.export"))).clicked()
                        && let Some(p) = rfd::FileDialog::new()
                            .add_filter("JSON", &["json"])
                            .set_file_name("rustrace-rules.json")
                            .save_file()
                    {
                        let json =
                            serde_json::to_string_pretty(&self.cfg.rules).unwrap_or_default();
                        if let Err(e) = std::fs::write(&p, json) {
                            self.status_error = Some(t!("error.export", error = e));
                        }
                    }
                    if ui.button(t!("rules.restore_defaults")).clicked() {
                        self.cfg.rules = Config::default_rules();
                        changed = true;
                    }
                });
            });
        self.show_rules = open;
        if changed {
            self.recompile_rules();
            self.cfg_dirty = true;
        }
    }

    fn colors_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_colors;
        let mut changed = false;
        let mut reload = false;
        egui::Window::new(t!("colors.title"))
            .id(egui::Id::unique("colors_window"))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                changed |= ui
                    .checkbox(&mut self.cfg.color_by_severity, t!("colors.by_severity"))
                    .changed();
                if ui
                    .checkbox(&mut self.cfg.keyword_severity, t!("colors.keyword_severity"))
                    .changed()
                {
                    changed = true;
                    reload = true;
                }
                ui.separator();
                let edit_dark = self.edit_dark;
                theme_selector(ui, &mut self.edit_dark);
                ui.add_space(4.0);
                egui::Grid::new("sev_colors").spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.strong("");
                    ui.strong(t!("rules.col.text"));
                    ui.strong(t!("rules.col.background"));
                    ui.strong(t!("rules.col.preview"));
                    ui.end_row();
                    for (key, fg, bg) in self.cfg.palette_mut(edit_dark) {
                        ui.label(t!(key));
                        changed |= color_button(ui, fg);
                        changed |= color_button(ui, bg);
                        preview(ui, edit_dark, Some(*fg), Some(*bg));
                        ui.end_row();
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t!("colors.restore_defaults")).clicked() {
                        self.cfg.reset_palette(edit_dark);
                        changed = true;
                    }
                    let other = theme_name(!edit_dark);
                    if ui
                        .button(format!("⇄ {}", t!("colors.copy_from", theme = other)))
                        .clicked()
                    {
                        let src = self.cfg.palette(!edit_dark);
                        let [e, w, s] = self.cfg.palette_mut(edit_dark);
                        for ((_, fg, bg), (sf, sb)) in
                            [e, w, s].into_iter().zip([src.error, src.warning, src.search])
                        {
                            *fg = sf;
                            *bg = sb;
                        }
                        changed = true;
                    }
                });
            });
        self.show_colors = open;
        if changed {
            self.cfg_dirty = true;
        }
        if reload {
            self.reload_full();
        }
    }

    /// Loading progress: big (middle of the window) or compact (status bar).
    fn progress_ui(&mut self, ui: &mut egui::Ui, big: bool) {
        let Some(l) = &self.loading else { return };
        let total = l.progress.total.load(Ordering::Relaxed);
        let done = l.progress.done.load(Ordering::Relaxed).min(total);
        // Short refreshes (a few kB) are not shown, so the status bar does not flicker.
        if !big && (l.started.elapsed() < Duration::from_millis(300) || total < 4 * 1024 * 1024) {
            return;
        }
        let frac = if total > 0 {
            done as f32 / total as f32
        } else {
            0.0
        };
        let text = t!(
            "progress.text",
            percent = format!("{:.0}", frac * 100.0),
            done = format_size(done),
            total = format_size(total)
        );
        let cancel = if big {
            ui.heading(t!("progress.title"));
            ui.add_space(8.0);
            ui.add(egui::ProgressBar::new(frac).text(text).animate(true));
            ui.add_space(8.0);
            ui.button(t!("progress.cancel")).clicked()
        } else {
            ui.spinner();
            ui.add(egui::ProgressBar::new(frac).text(text).desired_width(260.0));
            ui.small_button(t!("progress.cancel")).clicked()
        };
        if cancel {
            self.cancel_loading();
            self.cfg.auto_refresh = false;
            self.cfg_dirty = true;
            self.info(t!("progress.cancelled"));
        }
    }

    fn lookup_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_lookup;
        egui::Window::new(format!("🔎 {}", t!("lookup.title")))
            .id(egui::Id::unique("lookup_window"))
            .open(&mut open)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t!("lookup.code"));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.lookup_input)
                            .hint_text("0x80070005, 80070005, -2147024891, 1603 …")
                            .desired_width(280.0)
                            .font(TextStyle::Monospace),
                    );
                    if self.lookup_focus {
                        r.request_focus();
                        self.lookup_focus = false;
                    }
                    if !self.lookup_input.is_empty() && ui.small_button("✖").clicked() {
                        self.lookup_input.clear();
                    }
                });
                ui.separator();

                let input = self.lookup_input.trim().to_owned();
                if input.is_empty() {
                    ui.weak(t!("lookup.hint"));
                    return;
                }
                if self.lookup_cache.as_ref().is_none_or(|(k, _)| *k != input) {
                    let info = errcodes::parse_code(&input).map(errcodes::lookup);
                    self.lookup_cache = Some((input.clone(), info));
                }
                let Some((_, Some(info))) = &self.lookup_cache else {
                    ui.colored_label(RED, t!("lookup.invalid"));
                    return;
                };
                egui::Grid::new("code_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(t!("lookup.hex"));
                        ui.monospace(info.hex());
                        ui.end_row();
                        ui.label(t!("lookup.decimal"));
                        ui.monospace(t!(
                            "lookup.decimal.value",
                            signed = info.signed(),
                            unsigned = info.hresult
                        ));
                        ui.end_row();
                        if let Some(w) = info.win32_code() {
                            ui.label(t!("lookup.win32"));
                            ui.monospace(format!("{w}  (0x{w:X})"));
                            ui.end_row();
                        }
                        ui.label(t!("lookup.facility"));
                        ui.label(&info.facility);
                        ui.end_row();
                    });
                ui.separator();
                if info.descriptions.is_empty() {
                    ui.weak(t!("lookup.no_description"));
                    if !cfg!(windows) {
                        ui.weak(t!("lookup.windows_only"));
                    }
                }
                for (src, d) in &info.descriptions {
                    ui.strong(src);
                    ui.label(d);
                    ui.add_space(4.0);
                }
                if let Some((_, d)) = info.descriptions.first()
                    && ui.button(format!("📋 {}", t!("lookup.copy"))).clicked()
                {
                    ui.ctx().copy_text(format!("{} – {d}", info.hex()));
                }
            });
        self.show_lookup = open;
    }

    // ------------------------------------------------------------------
    // Export around highlighted rows
    // ------------------------------------------------------------------

    /// Is the entry "highlighted" according to the export criteria?
    fn is_export_hit(&self, idx: usize) -> bool {
        let e = self.entries.at(idx);
        let c = &self.cfg;
        (c.export_errors && e.severity == Severity::Error)
            || (c.export_warnings && e.severity == Severity::Warning)
            || (c.export_search && self.is_match.get(idx).copied().unwrap_or(false))
            || (c.export_rules
                && self.rules.iter().any(|r| {
                    r.regex.is_match(e.message)
                        || (r.rule.scope == HighlightScope::Row
                            && r.rule.match_component
                            && Self::other_columns_match(&r.regex, &e))
                }))
    }

    /// Indexes of highlighted entries (recomputed only when something they depend on changed).
    fn export_hits(&mut self) -> &[usize] {
        let key = ExportKey {
            data_gen: self.data_gen,
            rules_gen: self.rules_gen,
            search_gen: self.search_gen,
            len: self.entries.len(),
            rules: self.cfg.export_rules,
            errors: self.cfg.export_errors,
            warnings: self.cfg.export_warnings,
            search: self.cfg.export_search,
        };
        if self.export_hits.as_ref().is_none_or(|(k, _)| *k != key) {
            let hits: Vec<usize> = (0..self.entries.len())
                .filter(|&i| self.is_export_hit(i))
                .collect();
            self.export_hits = Some((key, hits));
        }
        &self.export_hits.as_ref().unwrap().1
    }

    /// Newest timestamp in the log (cached).
    fn latest_ts(&mut self) -> Option<String> {
        if self.latest_ts.as_ref().is_none_or(|(g, _)| *g != self.data_gen) {
            let latest = export::latest_timestamp(&self.entries).map(ms_to_iso);
            self.latest_ts = Some((self.data_gen, latest));
        }
        self.latest_ts.as_ref().and_then(|(_, t)| t.clone())
    }

    /// Computes what will be exported: highlighted rows → time filter → context ranges
    /// merged → older context clipped → only the last N blocks.
    fn export_plan(&mut self) -> ExportPlan {
        let (before, after, len) = (self.cfg.export_before, self.cfg.export_after, self.entries.len());
        let since = if self.cfg.export_since_enabled {
            match export::parse_datetime_input(&self.cfg.export_since, &self.date_format) {
                Some(iso) => Some(iso),
                None => {
                    return ExportPlan {
                        hits: Vec::new(),
                        blocks: Vec::new(),
                        since_invalid: true,
                    };
                }
            }
        } else {
            None
        };
        // "Rows visible in the table": every row shown in the table is a highlighted row
        // (the criteria are not used); the context still comes from the whole file.
        let all_hits = if self.cfg.export_visible_only {
            self.filtered.clone()
        } else {
            self.export_hits().to_vec()
        };
        let since = since.as_deref().and_then(iso_to_ms);
        let hits = export::filter_hits_since(&all_hits, &self.entries, since);
        let blocks = export::merge_ranges(&hits, before, after, len);
        let blocks = export::clip_blocks_since(&blocks, &self.entries, since);
        let last = self.cfg.export_last_enabled.then_some(self.cfg.export_last_blocks);
        let blocks = export::keep_last_blocks(blocks, last);
        // Count only hits that are really exported.
        let hits = hits
            .into_iter()
            .filter(|h| blocks.iter().any(|&(a, b)| (a..=b).contains(h)))
            .collect();
        ExportPlan {
            hits,
            blocks,
            since_invalid: false,
        }
    }

    /// Date/time formatted for the input field (without milliseconds).
    fn input_dt(&self, iso: &str) -> String {
        let s = self.display_dt(iso);
        match s.rfind('.') {
            Some(i) if s.len() - i == 4 => s[..i].to_owned(),
            _ => s,
        }
    }

    fn export_window(&mut self, ctx: &egui::Context) {
        if !self.show_export {
            return;
        }
        let mut open = true;
        let mut start = false;
        let mut changed = false;
        egui::Window::new(format!("📤 {}", t!("export.title")))
            .id(egui::Id::unique("export_window"))
            .open(&mut open)
            .resizable(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                egui::Grid::new("export_grid").spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(t!("export.before"));
                    changed |= ui
                        .add(egui::DragValue::new(&mut self.cfg.export_before).range(0..=100_000))
                        .changed();
                    ui.end_row();
                    ui.label(t!("export.after"));
                    changed |= ui
                        .add(egui::DragValue::new(&mut self.cfg.export_after).range(0..=100_000))
                        .changed();
                    ui.end_row();
                });
                ui.separator();
                ui.label(t!("export.include"));
                let (visible, total) = (self.filtered.len(), self.entries.len());
                changed |= ui
                    .checkbox(
                        &mut self.cfg.export_visible_only,
                        t!("export.visible_only", visible = visible, total = total),
                    )
                    .on_hover_text(t!("export.visible_only.tip"))
                    .changed();
                // The criteria are not used while every visible row counts as highlighted.
                ui.add_enabled_ui(!self.cfg.export_visible_only, |ui| {
                    changed |=
                        ui.checkbox(&mut self.cfg.export_rules, t!("export.inc.rules")).changed();
                    changed |=
                        ui.checkbox(&mut self.cfg.export_errors, t!("export.inc.errors")).changed();
                    changed |= ui
                        .checkbox(&mut self.cfg.export_warnings, t!("export.inc.warnings"))
                        .changed();
                    changed |= ui
                        .add_enabled(
                            self.search_re.is_some(),
                            egui::Checkbox::new(&mut self.cfg.export_search, t!("export.inc.search")),
                        )
                        .changed();
                });
                ui.separator();
                ui.label(t!("export.format"));
                changed |= ui
                    .radio_value(
                        &mut self.cfg.export_format,
                        ExportFormat::Original,
                        t!("export.format.original"),
                    )
                    .changed();
                changed |= ui
                    .radio_value(
                        &mut self.cfg.export_format,
                        ExportFormat::Readable,
                        t!("export.format.readable"),
                    )
                    .changed();
                changed |= ui
                    .checkbox(&mut self.cfg.export_separators, t!("export.separators"))
                    .changed();
                ui.separator();

                // --- Only records from a date/time on ---
                let latest = self.latest_ts();
                let example = self.input_dt(latest.as_deref().unwrap_or("2026-10-23 08:00:00.000"));
                ui.horizontal(|ui| {
                    changed |= ui
                        .checkbox(&mut self.cfg.export_since_enabled, t!("export.since"))
                        .changed();
                    changed |= ui
                        .add_enabled(
                            self.cfg.export_since_enabled,
                            egui::TextEdit::singleline(&mut self.cfg.export_since)
                                .hint_text(t!("export.since.hint", example = example))
                                .desired_width(190.0)
                                .font(TextStyle::Monospace),
                        )
                        .changed();
                });
                if self.cfg.export_since_enabled {
                    let mut preset: Option<String> = None;
                    ui.horizontal_wrapped(|ui| {
                        let sel_ts = self
                            .selected
                            .and_then(|i| self.entries.get(i))
                            .and_then(|e| e.timestamp())
                            .map(ms_to_iso);
                        if ui
                            .add_enabled(sel_ts.is_some(), egui::Button::new(t!("export.since.selected")))
                            .clicked()
                        {
                            preset = sel_ts;
                        }
                        for (hours, key) in [
                            (1, "export.since.last_hour"),
                            (24, "export.since.last_day"),
                            (24 * 7, "export.since.last_week"),
                        ] {
                            if ui
                                .add_enabled(latest.is_some(), egui::Button::new(t!(key)))
                                .on_hover_text(t!("export.since.presets.tip"))
                                .clicked()
                            {
                                preset = latest.as_deref().and_then(|l| export::hours_before(l, hours));
                            }
                        }
                    });
                    if let Some(iso) = preset {
                        self.cfg.export_since = self.input_dt(&iso);
                        changed = true;
                    }
                    if latest.is_none() {
                        ui.colored_label(ORANGE, t!("export.since.no_time"));
                    } else {
                        match export::parse_datetime_input(&self.cfg.export_since, &self.date_format) {
                            Some(iso) => {
                                ui.weak(t!("export.since.parsed", time = self.input_dt(&iso)));
                            }
                            None => {
                                ui.colored_label(RED, t!("export.since.invalid", example = example));
                            }
                        }
                    }
                }

                // --- Only the last N blocks ---
                ui.horizontal(|ui| {
                    changed |= ui
                        .checkbox(&mut self.cfg.export_last_enabled, t!("export.last"))
                        .changed();
                    changed |= ui
                        .add_enabled(
                            self.cfg.export_last_enabled,
                            egui::DragValue::new(&mut self.cfg.export_last_blocks).range(1..=100_000),
                        )
                        .changed();
                    ui.label(t!("export.last.unit"));
                });
                ui.separator();
                ui.weak(t!("export.note"));
                ui.add_space(4.0);

                let plan = self.export_plan();
                let since_invalid = plan.since_invalid;
                let hits = plan.hits.len();
                let blocks = plan.blocks;
                let rows = export::row_count(&blocks);
                ui.strong(t!(
                    "export.summary",
                    hits = hits,
                    blocks = blocks.len(),
                    rows = rows
                ));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let busy = self.export_job.is_some() || self.loading.is_some();
                    if ui
                        .add_enabled(rows > 0 && !busy, egui::Button::new(format!("💾 {}", t!("export.button"))))
                        .clicked()
                    {
                        start = true;
                    }
                    if self.export_job.is_some() {
                        ui.spinner();
                        ui.label(t!("export.running"));
                    } else if since_invalid {
                        ui.colored_label(RED, t!("export.since.fix"));
                    } else if rows == 0 {
                        ui.weak(t!("export.nothing"));
                    }
                });
            });
        if changed {
            self.cfg_dirty = true;
        }
        if start {
            self.start_export();
        }
        self.show_export = open;
    }

    /// Asks for the target file and writes the export in a background thread.
    fn start_export(&mut self) {
        let Some(src) = self.cur_path.clone() else { return };
        let plan = self.export_plan();
        let blocks = plan.blocks;
        let hits = plan.hits;
        if blocks.is_empty() {
            return;
        }
        let readable = self.cfg.export_format == ExportFormat::Readable;
        let stem = src
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "log".into());
        let (ext, filter) = if readable {
            ("txt", t!("dialog.filter.text"))
        } else {
            ("log", t!("dialog.filter.logs"))
        };
        let mut dlg = rfd::FileDialog::new()
            .add_filter(filter, &[ext])
            .set_file_name(format!("{stem}_export.{ext}"));
        if let Some(dir) = src.parent() {
            dlg = dlg.set_directory(dir);
        }
        let Some(target) = dlg.save_file() else { return };

        // Blocks split by file (a log merged from rotated parts); file names only then.
        let multi = self.files.len() > 1;
        let segs: Vec<Vec<export::Segment>> =
            blocks.iter().map(|&b| export::segments(&self.entries, b)).collect();
        let seg_sep = |s: &export::Segment| {
            let name = multi.then(|| self.file_label(s.file));
            export::separator(name.as_deref(), s.lines.0, s.lines.1)
        };
        let rows = export::row_count(&blocks);
        let separators = self.cfg.export_separators;
        // The readable text is built here (it needs translations and date formatting);
        // the original lines are read from the files in the thread.
        let readable_text = readable.then(|| {
            let block_seps: Vec<String> = segs
                .iter()
                .map(|parts| match parts.as_slice() {
                    [one] => seg_sep(one),
                    many => {
                        let label: Vec<String> = many
                            .iter()
                            .map(|s| format!("{}: {} – {}", self.file_label(s.file), s.lines.0, s.lines.1))
                            .collect();
                        format!("-------- {} --------", label.join(", "))
                    }
                })
                .collect();
            let row_blocks: Vec<Vec<ReadableRow<'_>>> = blocks
                .iter()
                .map(|&(a, b)| {
                    (a..=b)
                        .map(|i| {
                            let e = self.entries.at(i);
                            ReadableRow {
                                highlighted: hits.binary_search(&i).is_ok(),
                                line: if multi {
                                    format!("{}:{}", self.file_label(e.file), e.line_no)
                                } else {
                                    e.line_no.to_string()
                                },
                                datetime: self.display_dt(&e.datetime()),
                                severity: match e.severity {
                                    Severity::Error => t!("severity.error"),
                                    Severity::Warning => t!("severity.warning"),
                                    Severity::Info => t!("severity.info"),
                                },
                                entry: e,
                            }
                        })
                        .collect()
                })
                .collect();
            // Extra columns of a log definition are appended with their headers.
            let mut header = t!("export.header");
            for (i, name) in self.schema.extra_names.iter().enumerate() {
                let title = self
                    .schema
                    .columns
                    .iter()
                    .find(|c| c.kind == ColumnKind::Extra(i))
                    .and_then(|c| c.title.as_deref())
                    .unwrap_or(name);
                header.push('\t');
                header.push_str(title);
            }
            let extra_cols = self.schema.extra_names.len();
            export::build_readable(&header, &row_blocks, &block_seps, separators, extra_cols)
        });
        // Original lines: every part is read from its file in the thread; a part whose file
        // was overwritten by a later rotation falls back to the message texts.
        enum PartSrc {
            File { job: usize, range: usize },
            Text(Vec<String>),
        }
        let mut jobs: Vec<(PathBuf, Vec<(usize, usize)>)> = Vec::new();
        let mut parts: Vec<(String, PartSrc)> = Vec::new();
        if !readable {
            for s in segs.iter().flatten() {
                let src = match self.files.get(s.file as usize).and_then(|f| f.path.clone()) {
                    Some(path) => {
                        let job = match jobs.iter().position(|(p, _)| *p == path) {
                            Some(j) => j,
                            None => {
                                jobs.push((path, Vec::new()));
                                jobs.len() - 1
                            }
                        };
                        jobs[job].1.push(s.lines);
                        PartSrc::File { job, range: jobs[job].1.len() - 1 }
                    }
                    None => PartSrc::Text(
                        (s.entries.0..=s.entries.1)
                            .flat_map(|i| self.entries.message(i).lines().map(str::to_owned).collect::<Vec<_>>())
                            .collect(),
                    ),
                };
                parts.push((seg_sep(s), src));
            }
        }
        let nblocks = blocks.len();
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        let done_msg = t!(
            "export.done",
            rows = rows,
            blocks = nblocks,
            path = target.display()
        );
        let spawned = std::thread::Builder::new()
            .name("rustrace-export".into())
            .spawn(move || {
                let text = match readable_text {
                    Some(t) => Ok(t),
                    None => jobs
                        .iter()
                        .map(|(path, ranges)| crate::parser::read_line_ranges(path, ranges))
                        .collect::<std::io::Result<Vec<_>>>()
                        .map(|mut read| {
                            let parts: Vec<(String, Vec<String>)> = parts
                                .into_iter()
                                .map(|(sep, src)| match src {
                                    PartSrc::File { job, range } => {
                                        (sep, std::mem::take(&mut read[job][range]))
                                    }
                                    PartSrc::Text(t) => (sep, t),
                                })
                                .collect();
                            export::build_original(&parts, separators)
                        }),
                };
                let res = text
                    .and_then(|t| std::fs::write(&target, t))
                    .map(|_| done_msg)
                    .map_err(|e| e.to_string());
                let _ = tx.send(res);
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.export_job = Some(rx),
            Err(e) => self.status_error = Some(t!("error.thread", error = e)),
        }
    }

    fn check_export(&mut self) {
        let Some(rx) = &self.export_job else { return };
        match rx.try_recv() {
            Ok(Ok(msg)) => {
                self.export_job = None;
                self.status_error = None;
                self.status_info = Some((msg, Instant::now() + Duration::from_secs(5)));
            }
            Ok(Err(e)) => {
                self.export_job = None;
                self.status_error = Some(t!("error.export", error = e));
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.ctx.request_repaint_after(Duration::from_millis(100));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.export_job = None;
                self.status_error = Some(t!("error.export", error = "internal error"));
            }
        }
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        egui::Window::new(t!("help.title"))
            .id(egui::Id::unique("help_window"))
            .open(&mut self.show_help)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("keys").spacing([20.0, 4.0]).show(ui, |ui| {
                    for (k, d) in [
                        ("Ctrl+O", "help.open"),
                        ("F5", "help.refresh"),
                        ("Ctrl+F5", "help.reload"),
                        ("Ctrl+P", "help.pause"),
                        ("Ctrl+F", "help.search"),
                        ("Enter / F3", "help.next"),
                        ("Shift+Enter / Shift+F3", "help.prev"),
                        ("Ctrl+L", "help.filter"),
                        ("Ctrl+H", "help.rules"),
                        ("Ctrl+E", "help.lookup"),
                        ("Ctrl+Shift+S", "help.export"),
                        ("↑ ↓ PgUp PgDn Home", "help.navigate"),
                        ("End", "help.end"),
                        ("Ctrl+C", "help.copy"),
                        ("Ctrl + / Ctrl −", "help.zoom"),
                    ] {
                        ui.monospace(kb(k));
                        ui.label(t!(d));
                        ui.end_row();
                    }
                    ui.monospace(kb(&format!("Ctrl + {}", t!("key.wheel"))));
                    ui.label(t!("help.zoom_wheel"));
                    ui.end_row();
                    ui.monospace(kb(&format!("Shift + {}", t!("key.wheel"))));
                    ui.label(t!("help.hscroll"));
                    ui.end_row();
                });
                ui.separator();
                ui.label(t!("help.open_hint"));
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.dark = ui.visuals().dark_mode;
        if !self.show_rules && !self.show_colors {
            // Editors open with the colors of the theme in use.
            self.edit_dark = self.dark;
        }
        self.check_loading();
        self.handle_input(&ctx);
        self.tick_auto_refresh(&ctx);
        self.update_title(&ctx);

        self.hover_log = false;
        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        if self.cfg.show_details {
            egui::Panel::bottom("details")
                .resizable(true)
                .default_size(150.0)
                .min_size(60.0)
                .show(ui, |ui| self.details(ui));
        }
        egui::CentralPanel::default().show(ui, |ui| self.table(ui));
        self.handle_zoom(&ctx);

        self.rules_window(&ctx);
        self.colors_window(&ctx);
        self.help_window(&ctx);
        self.lookup_window(&ctx);
        self.export_window(&ctx);
        self.check_export();

        // Save settings (debounced, so we do not write on every mouse move).
        let closing = ctx.input(|i| i.viewport().close_requested());
        if self.cfg_dirty && (closing || self.last_save.elapsed() > Duration::from_millis(800)) {
            if let Err(e) = self.cfg.save() {
                self.status_error = Some(t!("error.save_settings", error = e));
            }
            self.cfg_dirty = false;
            self.last_save = Instant::now();
        } else if self.cfg_dirty {
            ctx.request_repaint_after(Duration::from_millis(900));
        }
    }
}

/// Color picker button for an RGBA array; returns true when changed.
fn color_button(ui: &mut egui::Ui, color: &mut [u8; 4]) -> bool {
    let mut c = c32(*color);
    let changed = ui.color_edit_button_srgba(&mut c).changed();
    if changed {
        *color = c.to_srgba_unmultiplied();
    }
    changed
}

/// Sample text drawn on the background of the given theme.
fn preview(ui: &mut egui::Ui, dark: bool, fg: Option<[u8; 4]>, bg: Option<[u8; 4]>) {
    let v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let mut text = RichText::new(format!(" {} ", t!("rules.preview_text")))
        .color(fg.map(c32).unwrap_or(v.text_color()));
    if let Some(bg) = bg {
        text = text.background_color(c32(bg));
    }
    egui::Frame::new()
        .fill(v.extreme_bg_color)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .corner_radius(3.0)
        .show(ui, |ui| {
            ui.label(text);
        });
}

fn theme_name(dark: bool) -> String {
    if dark {
        t!("menu.view.theme.dark")
    } else {
        t!("menu.view.theme.light")
    }
}

/// "Colors for theme: [Light] [Dark]" – returns true when the selection changed.
fn theme_selector(ui: &mut egui::Ui, edit_dark: &mut bool) -> bool {
    let before = *edit_dark;
    ui.horizontal(|ui| {
        ui.label(t!("colors.for_theme"));
        ui.selectable_value(edit_dark, false, format!("☀ {}", theme_name(false)));
        ui.selectable_value(edit_dark, true, format!("🌙 {}", theme_name(true)));
    });
    before != *edit_dark
}

/// Localizes modifier key names in a shortcut ("Ctrl+O" → "Strg+O" in German).
fn kb(shortcut: &str) -> String {
    shortcut
        .replace("Ctrl", &t!("key.ctrl"))
        .replace("Shift", &t!("key.shift"))
}

fn scope_label(s: HighlightScope) -> String {
    match s {
        HighlightScope::Row => t!("rules.scope.row"),
        HighlightScope::Text => t!("rules.scope.text"),
    }
}

/// Builds formatted text with highlighted pieces. Later spans take precedence.
fn build_job(text: &str, spans: &[Span], default_fg: Color32, font: &FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    if spans.is_empty() {
        job.append(
            text,
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: default_fg,
                ..Default::default()
            },
        );
        return job;
    }
    let mut cuts: Vec<usize> = Vec::with_capacity(spans.len() * 2 + 2);
    cuts.push(0);
    cuts.push(text.len());
    for s in spans {
        cuts.push(s.start);
        cuts.push(s.end);
    }
    cuts.sort_unstable();
    cuts.dedup();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= b {
            continue;
        }
        let mut fg = default_fg;
        let mut bg = Color32::TRANSPARENT;
        for s in spans {
            if s.start <= a && s.end >= b {
                if let Some(c) = s.fg {
                    fg = c;
                }
                if let Some(c) = s.bg {
                    bg = c;
                }
            }
        }
        job.append(
            &text[a..b],
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: fg,
                background: bg,
                ..Default::default()
            },
        );
    }
    job
}

fn format_size(b: u64) -> String {
    let b = b as f64;
    if b >= 1024.0 * 1024.0 {
        format!("{:.1} MB", b / 1024.0 / 1024.0)
    } else if b >= 1024.0 {
        format!("{:.1} kB", b / 1024.0)
    } else {
        format!("{b} B")
    }
}

fn trim_num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}
