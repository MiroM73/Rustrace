use crate::config::{Config, HighlightRule, HighlightScope, IntervalUnit};
use crate::errcodes::{self, CodeInfo};
use crate::export::{self, ExportFormat, ReadableRow};
use crate::i18n;
use crate::parser::{LogEntry, LogSource, PollResult, Progress, Severity};
use crate::t;
use eframe::egui::{
    self, text::LayoutJob, Align, Color32, FontId, Key, Layout, Modifiers, RichText, Sense,
    TextFormat, TextStyle,
};
use egui_extras::{Column, TableBuilder};
use regex::{Regex, RegexBuilder};
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
    entries: Vec<LogEntry>,
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
}

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
            entries: Vec::new(),
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
        };
        app.apply_theme(&cc.egui_ctx);
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

    fn entry_matches(re: &Regex, e: &LogEntry) -> bool {
        re.is_match(&e.message) || re.is_match(&e.component)
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
            let e = &self.entries[i];
            let m = self
                .search_re
                .as_ref()
                .is_some_and(|re| Self::entry_matches(re, e));
            self.is_match.push(m);
            let pass_search = !self.filter_mode || self.search_re.is_none() || m;
            if pass_search && self.sev_visible(e.severity) {
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
        self.entries = Vec::new();
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
                self.source = Some(src);
                self.apply_result(res);
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
                self.committed_len = self.entries.len();
                self.entries.extend(provisional);
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
                self.entries.extend(committed);
                self.committed_len = self.entries.len();
                self.entries.extend(provisional);
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
            if Self::entry_matches(&re, &self.entries[idx]) {
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
            ctx.copy_text(e.message.clone());
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
                        .add(egui::Slider::new(&mut self.cfg.font_size, 9.0..=24.0).step_by(0.5))
                        .changed()
                    {
                        self.cfg_dirty = true;
                    }
                });
                ui.separator();
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
        ui.add_space(2.0);
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
                if self.file_size > 0 {
                    ui.label(format_size(self.file_size));
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
            .map(|e| errcodes::find_codes(&e.message))
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
            if !e.component.is_empty() {
                ui.separator();
                ui.label(t!("details.component", component = e.component));
            }
            if !e.datetime.is_empty() {
                ui.separator();
                ui.label(t!("details.time", time = self.display_dt(&e.datetime)));
            }
            if !e.thread.is_empty() {
                ui.separator();
                ui.label(t!("details.thread", thread = e.thread));
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
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut s = e.message.as_str();
                ui.add(
                    egui::TextEdit::multiline(&mut s)
                        .desired_width(f32::INFINITY)
                        .font(TextStyle::Monospace),
                );
            });
        if open_code.is_some() {
            self.open_lookup(open_code);
        }
    }

    /// Whole-row colors: (foreground, background).
    fn row_colors(&self, e: &LogEntry) -> (Option<Color32>, Option<Color32>) {
        // "Whole row" rules take precedence – the first matching one wins.
        for r in &self.rules {
            if r.rule.scope != HighlightScope::Row {
                continue;
            }
            let hit = r.regex.is_match(&e.message)
                || (r.rule.match_component && r.regex.is_match(&e.component));
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

        let mut tb = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .stick_to_bottom(self.cfg.auto_scroll)
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::initial(60.0).at_least(40.0).clip(true))
            .column(Column::initial(720.0).at_least(120.0).clip(true))
            .column(Column::initial(150.0).at_least(40.0).clip(true))
            .column(Column::initial(175.0).at_least(40.0).clip(true))
            .column(Column::remainder().at_least(60.0).clip(true));
        if let Some(r) = self.scroll_to.take() {
            tb = tb.scroll_to_row(r, Some(Align::Center));
        }

        let mut clicked: Option<usize> = None;
        let mut action: Option<RowAction> = None;
        let headers = [
            t!("column.line"),
            t!("column.message"),
            t!("column.component"),
            t!("column.datetime"),
            t!("column.thread"),
        ];

        tb.header(22.0, |mut h| {
            for title in headers {
                h.col(|ui| {
                    ui.strong(title);
                });
            }
        })
        .body(|body| {
            body.rows(row_h, self.filtered.len(), |mut row| {
                let idx = self.filtered[row.index()];
                let e = &self.entries[idx];
                let is_sel = self.selected == Some(idx);
                row.set_selected(is_sel);

                let (fg, bg) = if is_sel {
                    (None, None)
                } else {
                    self.row_colors(e)
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
                row.col(|ui| simple(ui, &e.component));
                row.col(|ui| simple(ui, &self.display_dt(&e.datetime)));
                row.col(|ui| simple(ui, &e.thread));

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
                    let codes = errcodes::find_codes(&e.message);
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

        if let Some(i) = clicked {
            self.selected = Some(i);
        }
        if let Some(a) = action {
            let ctx = ui.ctx().clone();
            match a {
                RowAction::ExplainCode(v) => self.open_lookup(Some(v)),
                RowAction::CopyMessage(i) => ctx.copy_text(self.entries[i].message.clone()),
                RowAction::CopyRow(i) => {
                    let e = &self.entries[i];
                    ctx.copy_text(format!(
                        "{}\t{}\t{}\t{}",
                        e.message,
                        e.component,
                        self.display_dt(&e.datetime),
                        e.thread
                    ));
                }
                RowAction::SearchComponent(i) => {
                    self.search_text = self.entries[i].component.clone();
                    self.search_regex = false;
                    self.recompile_search();
                }
                RowAction::RuleFromComponent(i) => {
                    let comp = self.entries[i].component.clone();
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
        let cancel;
        if big {
            ui.heading(t!("progress.title"));
            ui.add_space(8.0);
            ui.add(egui::ProgressBar::new(frac).text(text).animate(true));
            ui.add_space(8.0);
            cancel = ui.button(t!("progress.cancel")).clicked();
        } else {
            ui.spinner();
            ui.add(egui::ProgressBar::new(frac).text(text).desired_width(260.0));
            cancel = ui.small_button(t!("progress.cancel")).clicked();
        }
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
        let e = &self.entries[idx];
        let c = &self.cfg;
        (c.export_errors && e.severity == Severity::Error)
            || (c.export_warnings && e.severity == Severity::Warning)
            || (c.export_search && self.is_match.get(idx).copied().unwrap_or(false))
            || (c.export_rules
                && self.rules.iter().any(|r| {
                    r.regex.is_match(&e.message)
                        || (r.rule.scope == HighlightScope::Row
                            && r.rule.match_component
                            && r.regex.is_match(&e.component))
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
            self.latest_ts = Some((self.data_gen, export::latest_timestamp(&self.entries)));
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
        let all_hits = self.export_hits().to_vec();
        let hits = export::filter_hits_since(&all_hits, &self.entries, since.as_deref());
        let blocks = export::merge_ranges(&hits, before, after, len);
        let blocks = export::clip_blocks_since(&blocks, &self.entries, since.as_deref());
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
                changed |= ui.checkbox(&mut self.cfg.export_rules, t!("export.inc.rules")).changed();
                changed |= ui.checkbox(&mut self.cfg.export_errors, t!("export.inc.errors")).changed();
                changed |= ui
                    .checkbox(&mut self.cfg.export_warnings, t!("export.inc.warnings"))
                    .changed();
                changed |= ui
                    .add_enabled(
                        self.search_re.is_some(),
                        egui::Checkbox::new(&mut self.cfg.export_search, t!("export.inc.search")),
                    )
                    .changed();
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
                            .and_then(|e| export::timestamp(e).map(str::to_owned));
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

        let line_ranges: Vec<(usize, usize)> = blocks
            .iter()
            .map(|&b| export::line_range(&self.entries, b))
            .collect();
        let rows = export::row_count(&blocks);
        let separators = self.cfg.export_separators;
        // The readable text is built here (it needs translations and date formatting);
        // the original lines are read from the file in the thread.
        let readable_text = readable.then(|| {
            let row_blocks: Vec<Vec<ReadableRow<'_>>> = blocks
                .iter()
                .map(|&(a, b)| {
                    (a..=b)
                        .map(|i| {
                            let e = &self.entries[i];
                            ReadableRow {
                                highlighted: hits.binary_search(&i).is_ok(),
                                line: e.line_no,
                                datetime: self.display_dt(&e.datetime),
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
            export::build_readable(&t!("export.header"), &row_blocks, &line_ranges, separators)
        });
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
                    None => crate::parser::read_line_ranges(&src, &line_ranges)
                        .map(|lines| export::build_original(&line_ranges, &lines, separators)),
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
