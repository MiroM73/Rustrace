use crate::colfilter::{ColumnFilter, DateFilter, DateLayout};
use crate::config::{
    Config, DetailsView, HighlightRule, HighlightScope, IntervalUnit, UpdateCheck,
};
use crate::errcodes::{self, CodeInfo};
use crate::excerpt;
use crate::export::{self, ExportFormat, ReadableRow};
#[cfg(windows)]
use crate::fileassoc::{self, ExtStatus};
use crate::filter::{
    CellFilter, FilterOutput, FilterSpec, MatchBits, entry_matches, other_columns_match,
};
use crate::i18n;
use crate::logdef::{self, Column as LogColumn, ColumnKind, LogDef, Role, Schema};
use crate::merge::{self, LogSet, ReadResult};
use crate::parser::{FormatChoice, Progress, Severity, SourceFile};
use crate::store::{EntryRef, LogStore, iso_to_ms, ms_to_iso};
use crate::t;
use crate::update::{self, FormatState, Release, SessionState, Updates, WindowState};
use eframe::egui::{
    self, Align, Color32, FontId, Key, Layout, Modifiers, RichText, Sense, TextFormat, TextStyle,
    text::LayoutJob,
};
use egui_extras::{Column, TableBuilder};
use regex::{Regex, RegexBuilder};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

// The `impl App` blocks are split by area; every submodule starts with `use crate::app::*;`.
mod bookmarks;
mod details;
mod dialogs;
mod filter;
mod input;
mod loading;
mod logdefs;
mod menu;
mod statusbar;
mod table;
mod toolbar;
mod widgets;

use statusbar::*;
use table::*;
use widgets::*;

/// Program version from Cargo.toml (window title, About window).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Project page on GitHub (Cargo.toml `repository`).
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// Window title without a file: program name and version.
pub fn base_title() -> String {
    format!("RusTrace {VERSION}")
}

/// Max. number of message characters shown in the table (the full message is in the details pane).
const MAX_CELL_CHARS: usize = 2000;
/// A typed search text is searched right away from this many characters on (shorter: Enter).
const INSTANT_SEARCH_MIN_CHARS: usize = 3;
/// Logs with more entries are re-filtered in a background thread (#16).
const SYNC_FILTER_MAX_ENTRIES: usize = 100_000;
/// The "Searching…" hint shows only for a background re-filter that takes longer than this.
const FILTER_HINT_DELAY: Duration = Duration::from_millis(200);

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
    rx: mpsc::Receiver<(LogSet, std::io::Result<ReadResult>)>,
    progress: Arc<Progress>,
    started: Instant,
}

/// Full re-filter of a big log in a background thread (#16).
struct FilterJob {
    /// `None` is never sent (a cancelled run sends nothing).
    rx: mpsc::Receiver<Option<FilterOutput>>,
    cancel: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<()>,
    started: Instant,
}

/// What waits for the running background re-filter.
#[derive(Default)]
struct AfterFilter {
    select_first: bool,
    scroll_end: bool,
    scroll_selected: bool,
}

/// Update check and installation (#15, #31).
enum UpdateState {
    Idle,
    /// Asking GitHub; `silent` = automatic check at start-up (no window unless a new version).
    Checking {
        rx: mpsc::Receiver<Result<Updates, String>>,
        silent: bool,
    },
    Failed(String),
    /// The latest release (newer or not) and the missed ones.
    Checked(Updates),
    /// Download + installation; `exe` = the program path before it was renamed.
    Installing {
        rx: mpsc::Receiver<Result<Vec<String>, String>>,
        progress: Arc<update::Progress>,
        release: Release,
        exe: PathBuf,
    },
    InstallFailed {
        error: String,
        release: Release,
    },
}

enum RowAction {
    ExplainCode(u32),
    ToggleBookmark(usize),
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
    source: Option<LogSet>,
    loading: Option<Loading>,
    /// The open log (the first one of a merged view) and all logs of the view (#21).
    cur_path: Option<PathBuf>,
    paths: Vec<PathBuf>,
    /// The log of every file (index = file id) and the log definition of every log.
    file_sources: Vec<usize>,
    source_defs: Vec<Option<Arc<LogDef>>>,
    /// The logs of a merged view are read with different formats.
    mixed_formats: bool,
    encoding_name: &'static str,
    file_size: u64,
    /// The file is added to recent files only after it has been read successfully.
    pending_recent: Option<String>,
    /// Shared with a background re-filter while one runs (`App::entries_mut` changes it).
    entries: Arc<LogStore>,
    /// Number of "final" entries; provisional ones follow (incomplete last line).
    committed_len: usize,
    /// Indexes of the displayed entries (after filtering).
    filtered: Vec<usize>,
    /// For every entry: does it match the search? (Keeps the match count.)
    is_match: MatchBits,
    /// Background re-filter in progress, cancelled ones that may still hold the entries, and
    /// what waits for its result.
    filter_job: Option<FilterJob>,
    stale_filter_jobs: Vec<std::thread::JoinHandle<()>>,
    after_filter: AfterFilter,
    /// A cancelled re-filter has to start again from the beginning.
    filter_restart: bool,
    /// (entry, search generation, view) the details pane was last scrolled to the first match for.
    details_scrolled: Option<(usize, u64, DetailsView)>,

    last_refresh: Option<Instant>,
    last_refresh_wall: Option<chrono::DateTime<chrono::Local>>,
    status_error: Option<String>,
    status_info: Option<(String, Instant)>,

    search_text: String,
    /// A search text shorter than `INSTANT_SEARCH_MIN_CHARS` was searched explicitly (Enter, F3,
    /// history, …); typing clears it.
    search_confirmed: bool,
    search_regex: bool,
    search_case: bool,
    filter_mode: bool,
    search_re: Option<Regex>,
    search_err: Option<String>,
    show_sev: [bool; 3], // info, warning, error

    selected: Option<usize>,
    scroll_to: Option<usize>,

    /// Bookmarks (#22): note by (file id, line number), the bookmarked entries (sorted), a change
    /// counter, the "only bookmarked rows" filter and the bookmarks window.
    bookmarks: HashMap<(u16, usize), String>,
    bookmark_rows: Vec<usize>,
    bookmarks_gen: u64,
    bookmarks_only: bool,
    show_bookmarks: bool,
    /// Files of the view before it was opened again; the next full read moves the bookmarks to
    /// the new file ids.
    bookmark_files: Option<Vec<SourceFile>>,

    rules: Vec<CompiledRule>,
    rule_errors: Vec<Option<String>>,

    show_rules: bool,
    show_colors: bool,
    show_help: bool,
    show_about: bool,
    /// Default app window (Windows); the state is read when the window opens.
    #[cfg(windows)]
    assoc: Option<fileassoc::Info>,
    /// New / changed predefined log definitions since the version the user has seen.
    def_updates: Vec<logdef::DefUpdate>,
    show_def_updates: bool,
    /// Texts of the predefined and own definitions (for the notice above).
    predefined_defs: std::collections::BTreeMap<String, String>,
    own_defs: std::collections::BTreeMap<String, (PathBuf, String)>,
    focus_search: bool,
    title: String,

    /// Error codes found in the selected row (entry index, codes).
    code_cache: Option<(usize, Vec<CodeInfo>)>,
    /// Raw line(s) of the selected row read from the file: (entry, data generation, text or
    /// why it is not available).
    raw_cache: Option<(usize, u64, Result<Arc<str>, String>)>,
    show_lookup: bool,
    lookup_input: String,
    lookup_focus: bool,
    lookup_cache: Option<(String, Option<CodeInfo>)>,

    /// Date pattern of the current language, e.g. "{d}.{m}.{y}", and how it is displayed.
    date_format: String,
    date_layout: DateLayout,
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
    col_filters: Vec<(ColumnKind, CellFilter)>,
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
    /// Frames drawn so far (up to 2): the start-up splash closes once the window shows.
    frames: u8,
    update: UpdateState,
    show_update: bool,
    /// The releases newer than this program, when there are any (status bar button).
    update_available: Option<Updates>,
    /// Selection and column filters of the previous version, applied after the first read.
    pending_restore: Option<SessionState>,
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
    bookmarks_gen: u64,
    len: usize,
    rules: bool,
    errors: bool,
    warnings: bool,
    search: bool,
    bookmarks: bool,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        paths: Vec<String>,
        restore: Option<SessionState>,
    ) -> Self {
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
            paths: Vec::new(),
            file_sources: Vec::new(),
            source_defs: Vec::new(),
            mixed_formats: false,
            encoding_name: "",
            file_size: 0,
            pending_recent: None,
            entries: Arc::new(LogStore::new()),
            committed_len: 0,
            filtered: Vec::new(),
            is_match: MatchBits::default(),
            filter_job: None,
            stale_filter_jobs: Vec::new(),
            after_filter: AfterFilter::default(),
            filter_restart: false,
            details_scrolled: None,
            last_refresh: None,
            last_refresh_wall: None,
            status_error: None,
            status_info: None,
            search_text: String::new(),
            search_confirmed: false,
            search_regex: false,
            search_case: false,
            filter_mode: false,
            search_re: None,
            search_err: None,
            show_sev: [true; 3],
            selected: None,
            scroll_to: None,
            bookmarks: HashMap::new(),
            bookmark_rows: Vec::new(),
            bookmarks_gen: 0,
            bookmarks_only: false,
            show_bookmarks: false,
            bookmark_files: None,
            rules: Vec::new(),
            rule_errors: Vec::new(),
            show_rules: false,
            show_colors: false,
            show_help: false,
            show_about: false,
            #[cfg(windows)]
            assoc: None,
            def_updates: Vec::new(),
            show_def_updates: false,
            predefined_defs: Default::default(),
            own_defs: Default::default(),
            focus_search: false,
            title: String::new(),
            code_cache: None,
            raw_cache: None,
            show_lookup: false,
            lookup_input: String::new(),
            lookup_focus: false,
            lookup_cache: None,
            date_format: t!("format.date"),
            date_layout: DateLayout::new(&t!("format.date")),
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
            frames: 0,
            update: UpdateState::Idle,
            show_update: false,
            update_available: None,
            pending_restore: None,
        };
        app.load_defs();
        app.check_def_updates();
        app.apply_theme(&cc.egui_ctx);
        let zoom = app
            .cfg
            .ui_zoom
            .clamp(*UI_ZOOM_RANGE.start(), *UI_ZOOM_RANGE.end());
        cc.egui_ctx.set_zoom_factor(zoom);
        app.recompile_rules();
        if !paths.is_empty() {
            app.open_files(paths.into_iter().map(PathBuf::from).collect());
        }
        let restored = restore.is_some();
        if let Some(state) = restore {
            app.restore_session(state);
        }
        // Automatic update check at every start / once a day (not right after an update).
        let now = chrono::Local::now().timestamp();
        if !restored && app.cfg.update_when.due(app.cfg.update_last_check, now) {
            app.start_update_check(true);
        }
        app
    }

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
                let zoom =
                    (ctx.zoom_factor() * delta).clamp(*UI_ZOOM_RANGE.start(), *UI_ZOOM_RANGE.end());
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
        self.date_layout = DateLayout::new(&self.date_format);
        self.code_cache = None;
        self.lookup_cache = None;
        self.cfg_dirty = true;
        // The date order of the time range texts and of the date column filter depends on the
        // language.
        self.apply_time_range();
        if !self.col_filters.is_empty() {
            self.apply_column_filters();
        }
        let files = std::mem::take(&mut self.files);
        self.set_files(files);
    }

    fn info(&mut self, s: String) {
        self.status_info = Some((s, Instant::now()));
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let names: Vec<String> = self
            .paths
            .iter()
            .map(|p| {
                p.file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect();
        let t = if names.is_empty() {
            base_title()
        } else {
            format!("{} – {}", base_title(), names.join(" + "))
        };
        if t != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(t.clone()));
            self.title = t;
        }
    }
}

/// Target platform and build profile, e.g. "windows x86_64, release".
fn build_info() -> String {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    format!(
        "{} {}, {profile}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.dark = ui.visuals().dark_mode;
        if !self.show_rules && !self.show_colors {
            // Editors open with the colors of the theme in use.
            self.edit_dark = self.dark;
        }
        self.check_filter_job();
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
        self.about_window(&ctx);
        #[cfg(windows)]
        self.assoc_window(&ctx);
        self.lookup_window(&ctx);
        self.bookmarks_window(&ctx);
        self.export_window(&ctx);
        self.def_updates_window(&ctx);
        self.update_window(&ctx);
        self.poll_update(&ctx);
        self.check_export();

        // The window becomes visible after its first frame; the splash goes with the second one.
        if self.frames < 2 {
            self.frames += 1;
            if self.frames == 2 {
                crate::splash::close();
            } else {
                ctx.request_repaint();
            }
        }

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
