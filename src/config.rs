//! Application settings – stored as JSON in the user's configuration folder
//! (Windows: %APPDATA%\RusTrace\config.json).

use crate::export::ExportFormat;
use crate::t;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// What the details pane shows for the selected entry.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum DetailsView {
    /// The parsed message.
    #[default]
    Message,
    /// The line(s) of the entry exactly as they are in the file.
    Raw,
}

/// What the Δ time column measures (#23).
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeltaMode {
    /// Since the previous row shown in the table (respects the filters).
    #[default]
    Visible,
    /// Since the previous row of the log, whatever the filters show.
    Log,
    /// Since the row set as the time reference.
    Reference,
}

/// The overview strip next to the table's scroll bar and the marks it shows (#24).
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct OverviewSettings {
    pub enabled: bool,
    pub errors: bool,
    pub warnings: bool,
    pub search: bool,
    pub bookmarks: bool,
}

impl Default for OverviewSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            errors: true,
            warnings: true,
            search: true,
            bookmarks: true,
        }
    }
}

/// When RusTrace looks for a new version at start-up.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateCheck {
    /// At every start.
    #[default]
    Start,
    /// At most once a day.
    Daily,
    Never,
}

impl UpdateCheck {
    pub const ALL: [UpdateCheck; 3] = [UpdateCheck::Start, UpdateCheck::Daily, UpdateCheck::Never];

    /// The automatic check runs now (`last` / `now` in Unix seconds).
    pub fn due(self, last: i64, now: i64) -> bool {
        match self {
            UpdateCheck::Start => true,
            UpdateCheck::Daily => now - last >= 24 * 3600,
            UpdateCheck::Never => false,
        }
    }

    pub fn label(self) -> String {
        match self {
            UpdateCheck::Start => t!("update.when.start"),
            UpdateCheck::Daily => t!("update.when.daily"),
            UpdateCheck::Never => t!("update.when.never"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum IntervalUnit {
    Ms,
    Sec,
    Min,
}

impl IntervalUnit {
    pub const ALL: [IntervalUnit; 3] = [IntervalUnit::Ms, IntervalUnit::Sec, IntervalUnit::Min];

    pub fn label(self) -> &'static str {
        match self {
            IntervalUnit::Ms => "ms",
            IntervalUnit::Sec => "s",
            IntervalUnit::Min => "min",
        }
    }

    pub fn to_ms(self, v: f64) -> f64 {
        match self {
            IntervalUnit::Ms => v,
            IntervalUnit::Sec => v * 1000.0,
            IntervalUnit::Min => v * 60_000.0,
        }
    }

    /// Allowed range of values for the unit.
    pub fn range(self) -> std::ops::RangeInclusive<f64> {
        match self {
            IntervalUnit::Ms => 50.0..=600_000.0,
            IntervalUnit::Sec => 0.05..=3600.0,
            IntervalUnit::Min => 0.01..=1440.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum HighlightScope {
    /// Colors the whole table row.
    Row,
    /// Colors only the matched text in the Message column.
    Text,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct HighlightRule {
    pub enabled: bool,
    pub name: String,
    pub pattern: String,
    pub is_regex: bool,
    pub case_sensitive: bool,
    pub scope: HighlightScope,
    pub use_fg: bool,
    pub fg: [u8; 4],
    pub use_bg: bool,
    pub bg: [u8; 4],
    /// Colors used with the dark theme (`fg`/`bg` are for the light theme).
    /// Alpha 0 means "not set yet" – filled from the light colors after loading.
    #[serde(default = "unset")]
    pub fg_dark: [u8; 4],
    #[serde(default = "unset")]
    pub bg_dark: [u8; 4],
    /// Also match against the Component column.
    pub match_component: bool,
    /// Severity given to the rows the rule matches (the first matching rule with a severity
    /// wins); `None` = unchanged (#50).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<RuleSeverity>,
}

/// The severity a highlight rule can set (#50).
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum RuleSeverity {
    Info,
    Warning,
    Error,
}

/// Marker for a color that has not been set (fully transparent).
const UNSET: [u8; 4] = [0, 0, 0, 0];

fn unset() -> [u8; 4] {
    UNSET
}

impl HighlightRule {
    /// (text, background) colors for the light or dark theme.
    pub fn colors(&self, dark: bool) -> ([u8; 4], [u8; 4]) {
        if dark {
            (self.fg_dark, self.bg_dark)
        } else {
            (self.fg, self.bg)
        }
    }

    pub fn colors_mut(&mut self, dark: bool) -> (&mut [u8; 4], &mut [u8; 4]) {
        if dark {
            (&mut self.fg_dark, &mut self.bg_dark)
        } else {
            (&mut self.fg, &mut self.bg)
        }
    }

    /// Copies the colors of one theme to the other.
    pub fn copy_colors(&mut self, from_dark: bool) {
        let (fg, bg) = self.colors(from_dark);
        let (tf, tb) = self.colors_mut(!from_dark);
        *tf = fg;
        *tb = bg;
    }

    /// Rules saved by an older version have no dark colors – use the light ones.
    pub fn fill_missing_dark(&mut self) {
        if self.fg_dark == UNSET {
            self.fg_dark = self.fg;
        }
        if self.bg_dark == UNSET {
            self.bg_dark = self.bg;
        }
    }
}

/// A named set of highlight rules (#70). The rules of every active set apply, the sets in
/// their order (the first matching rule wins).
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct RuleSet {
    pub name: String,
    /// Switched on; a bound set also needs an open log with one of its definitions.
    pub enabled: bool,
    /// Ids of the log definitions the set is bound to; empty = applies to every log.
    pub logdefs: Vec<String>,
    pub rules: Vec<HighlightRule>,
}

impl Default for RuleSet {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            logdefs: Vec::new(),
            rules: Vec::new(),
        }
    }
}

impl RuleSet {
    pub fn new(name: String, rules: Vec<HighlightRule>) -> Self {
        Self {
            name,
            rules,
            ..Self::default()
        }
    }

    /// The set applies: it is switched on and either not bound, or one of the open logs uses
    /// a log definition it is bound to (`def_ids` = ids of the definitions of the open logs).
    pub fn is_active(&self, def_ids: &[String]) -> bool {
        self.enabled
            && (self.logdefs.is_empty()
                || self
                    .logdefs
                    .iter()
                    .any(|d| def_ids.iter().any(|o| o.eq_ignore_ascii_case(d))))
    }
}

/// The file of an exported set (#70): the rules plus the name and the binding.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct RuleSetFile {
    pub name: String,
    #[serde(default)]
    pub logdefs: Vec<String>,
    pub rules: Vec<HighlightRule>,
}

/// What importing highlight rules did (#51).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RuleImport {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
}

/// Adds imported highlight rules to `rules` without creating duplicates (#51): a rule with the
/// same name and pattern as an existing one replaces it in place when any other setting differs
/// (colors, scope, regex, …; the existing rule keeps its On state) and is skipped when it is the
/// same. Other rules are appended in the order of the file.
pub fn import_rules(rules: &mut Vec<HighlightRule>, imported: Vec<HighlightRule>) -> RuleImport {
    let mut res = RuleImport::default();
    for mut r in imported {
        r.fill_missing_dark();
        match rules
            .iter_mut()
            .find(|e| e.name == r.name && e.pattern == r.pattern)
        {
            Some(e) => {
                r.enabled = e.enabled;
                if *e == r {
                    res.skipped += 1;
                } else {
                    *e = r;
                    res.updated += 1;
                }
            }
            None => {
                rules.push(r);
                res.added += 1;
            }
        }
    }
    res
}

impl Default for HighlightRule {
    fn default() -> Self {
        Self {
            enabled: true,
            name: t!("rule.new_name"),
            pattern: String::new(),
            is_regex: false,
            case_sensitive: false,
            scope: HighlightScope::Row,
            use_fg: true,
            fg: [0, 0, 0, 255],
            use_bg: true,
            bg: [255, 200, 120, 255],
            fg_dark: [255, 255, 255, 255],
            bg_dark: [150, 90, 20, 255],
            match_component: false,
            severity: None,
        }
    }
}

/// Severity, search and bookmark colors of one theme: (text, background).
pub struct Palette {
    pub error: ([u8; 4], [u8; 4]),
    pub warning: ([u8; 4], [u8; 4]),
    pub search: ([u8; 4], [u8; 4]),
    /// The line number cell of a bookmarked row (#22).
    pub bookmark: ([u8; 4], [u8; 4]),
    /// A Δ time cell with a long gap (#23).
    pub gap: ([u8; 4], [u8; 4]),
}

impl Palette {
    /// All (text, background) pairs, in the order of `Config::palette_mut`.
    pub fn pairs(&self) -> [([u8; 4], [u8; 4]); 5] {
        [
            self.error,
            self.warning,
            self.search,
            self.bookmark,
            self.gap,
        ]
    }
}

/// How many search texts the search history keeps.
pub const SEARCH_HISTORY_MAX: usize = 30;
/// How many remote computer names the log sets window remembers.
pub const REMOTE_COMPUTERS_MAX: usize = 15;

/// Position and size (logical pixels) of a window opened as its own OS window (#68).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolWindowState {
    pub pos: Option<[f32; 2]>,
    pub size: Option<[f32; 2]>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Config {
    /// Open the tool windows as their own OS windows that can leave the main window (#68).
    pub separate_windows: bool,
    /// Geometry of the tool windows by their key (#68, #69).
    pub windows: std::collections::BTreeMap<String, ToolWindowState>,
    /// Highlight rules before rule sets existed (#70): read for migration only, moved into the
    /// first set by `normalize_rule_sets` and never written again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<HighlightRule>,
    /// The highlight rule sets (#70); there is always at least one after loading. No serde
    /// default of the struct: an old file without the key must not get the example set.
    #[serde(default)]
    pub rule_sets: Vec<RuleSet>,
    /// `normalize_rule_sets` just created the first set from the old rules: the program gives it
    /// the name in the chosen language once the language is set (not saved).
    #[serde(skip)]
    pub rule_sets_migrated: bool,
    pub refresh_value: f64,
    pub refresh_unit: IntervalUnit,
    /// true = auto refresh running, false = paused.
    pub auto_refresh: bool,
    /// Stick to the end of the file (like "tail -f").
    pub auto_scroll: bool,
    /// Severity from the words error/fail/warn for entries without a type.
    pub keyword_severity: bool,
    pub color_by_severity: bool,
    pub error_fg: [u8; 4],
    pub error_bg: [u8; 4],
    pub warning_fg: [u8; 4],
    pub warning_bg: [u8; 4],
    pub search_fg: [u8; 4],
    pub search_bg: [u8; 4],
    pub error_fg_dark: [u8; 4],
    pub error_bg_dark: [u8; 4],
    pub warning_fg_dark: [u8; 4],
    pub warning_bg_dark: [u8; 4],
    pub search_fg_dark: [u8; 4],
    pub search_bg_dark: [u8; 4],
    pub bookmark_fg: [u8; 4],
    pub bookmark_bg: [u8; 4],
    pub bookmark_fg_dark: [u8; 4],
    pub bookmark_bg_dark: [u8; 4],
    pub gap_fg: [u8; 4],
    pub gap_bg: [u8; 4],
    pub gap_fg_dark: [u8; 4],
    pub gap_bg_dark: [u8; 4],
    pub recent_files: Vec<String>,
    /// Previously used search texts, newest first.
    pub search_history: Vec<String>,
    pub show_details: bool,
    /// Message or raw line(s) in the details pane.
    pub details_view: DetailsView,
    /// Show the filter row under the column headers.
    pub show_column_filters: bool,
    /// Show the time range (from – to) filter row.
    pub show_time_range: bool,
    /// The Δ time column, what it measures and the gap (seconds) from which it is colored.
    pub delta_column: bool,
    pub delta_mode: DeltaMode,
    pub delta_highlight: bool,
    pub delta_threshold_s: f64,
    /// Overview strip next to the scroll bar.
    pub overview: OverviewSettings,
    /// "system" | "dark" | "light"
    pub theme: String,
    /// Font size of the log text (table and message in the details pane).
    pub font_size: f32,
    /// Zoom factor of the whole user interface (1.0 = 100 %).
    pub ui_zoom: f32,
    /// UI language code ("en", "sk", …); empty = same as the operating system.
    pub language: String,

    /// Export around highlighted rows: context lines before / after each highlighted row.
    pub export_before: usize,
    pub export_after: usize,
    /// Which rows count as highlighted for the export.
    pub export_rules: bool,
    pub export_errors: bool,
    pub export_warnings: bool,
    pub export_search: bool,
    pub export_bookmarks: bool,
    pub export_format: ExportFormat,
    pub export_separators: bool,
    /// Export only records from this date/time on (text as entered by the user).
    pub export_since_enabled: bool,
    pub export_since: String,
    /// Export only the last N blocks.
    pub export_last_enabled: bool,
    pub export_last_blocks: usize,
    /// Every row visible in the table (search filter, severity toggles) is a highlighted row;
    /// the rules / errors / warnings / search criteria are not used. Context lines still come from the whole file.
    pub export_visible_only: bool,
    /// Table export – CSV / Excel (#32).
    pub table_export: crate::tabexport::Settings,

    /// Keep the message text of files of at least `temp_file_threshold_mb` MB in a memory-mapped
    /// temporary file instead of the process memory.
    pub temp_file_enabled: bool,
    pub temp_file_threshold_mb: u32,
    /// Also read the rotated older parts of a log (`name.lo_`, `name-YYYYMMDD-HHMMSS.log`) and keep
    /// the history when the log is rotated while it is watched.
    pub merge_rotated: bool,
    /// A merged view of several logs marks the File column with a color per log.
    pub color_files: bool,
    /// Memory limit of a (long watched) log: when the message text exceeds `max_log_mb` MB, the
    /// oldest entries are removed (down to three quarters of the limit).
    pub max_log_enabled: bool,
    pub max_log_mb: u32,
    /// Which log definitions are used: "all", "predefined" (built-in + next to the program) or
    /// "own" (the user's folder).
    pub logdef_sources: String,
    /// The predefined log definitions the user has seen (file name → text): after an update of
    /// RusTrace the new / changed ones are reported against these.
    pub known_logdefs: BTreeMap<String, String>,
    /// The log set chosen last in File → Open log set (`LogSetDef::key`, #27).
    pub last_log_set: String,
    /// Open log set on a remote computer (#27) and the recently used computer names, newest first.
    pub log_set_remote: bool,
    pub remote_computers: Vec<String>,

    /// When to look for a new version on GitHub at start-up.
    pub update_when: UpdateCheck,
    /// The on / off switch of versions before 0.9.1 (read only for the migration to `update_when`;
    /// old versions ignore `update_when` and keep checking once a day).
    #[serde(rename = "update_check", skip_serializing)]
    pub update_check_old: Option<bool>,
    /// Time of the last automatic check (Unix seconds).
    pub update_last_check: i64,
    /// A version the user chose to skip (no notice for it at start-up).
    pub update_skipped: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            separate_windows: true,
            windows: Default::default(),
            rules: Vec::new(),
            rule_sets: vec![RuleSet::new(t!("rules.set.default"), Self::default_rules())],
            rule_sets_migrated: false,
            refresh_value: 500.0,
            refresh_unit: IntervalUnit::Ms,
            auto_refresh: true,
            auto_scroll: true,
            keyword_severity: true,
            color_by_severity: true,
            // Colors as in CMTrace: error = red background, warning = yellow.
            error_fg: [255, 255, 0, 255],
            error_bg: [200, 30, 30, 255],
            warning_fg: [0, 0, 0, 255],
            warning_bg: [255, 235, 60, 255],
            search_fg: [0, 0, 0, 255],
            search_bg: [255, 150, 0, 255],
            // Darker, less glaring variants for the dark theme.
            error_fg_dark: [255, 225, 120, 255],
            error_bg_dark: [130, 25, 25, 255],
            warning_fg_dark: [255, 240, 190, 255],
            warning_bg_dark: [110, 90, 10, 255],
            search_fg_dark: [0, 0, 0, 255],
            search_bg_dark: [240, 150, 30, 255],
            bookmark_fg: [255, 255, 255, 255],
            bookmark_bg: [40, 110, 220, 255],
            bookmark_fg_dark: [255, 255, 255, 255],
            bookmark_bg_dark: [50, 110, 210, 255],
            gap_fg: [0, 0, 0, 255],
            gap_bg: [255, 180, 90, 255],
            gap_fg_dark: [255, 235, 210, 255],
            gap_bg_dark: [150, 75, 20, 255],
            recent_files: Vec::new(),
            search_history: Vec::new(),
            show_details: true,
            details_view: DetailsView::Message,
            show_column_filters: false,
            show_time_range: false,
            delta_column: false,
            delta_mode: DeltaMode::Visible,
            delta_highlight: true,
            delta_threshold_s: 30.0,
            overview: OverviewSettings::default(),
            theme: "system".into(),
            font_size: 13.0,
            ui_zoom: 1.0,
            language: String::new(),
            export_before: 5,
            export_after: 5,
            export_rules: true,
            export_errors: true,
            export_warnings: true,
            export_search: false,
            export_bookmarks: true,
            export_format: ExportFormat::Original,
            export_separators: true,
            export_since_enabled: false,
            export_since: String::new(),
            export_last_enabled: false,
            export_last_blocks: 10,
            export_visible_only: false,
            table_export: crate::tabexport::Settings::default(),
            temp_file_enabled: true,
            temp_file_threshold_mb: 100,
            merge_rotated: true,
            color_files: true,
            max_log_enabled: true,
            max_log_mb: 1000,
            logdef_sources: "all".into(),
            known_logdefs: BTreeMap::new(),
            last_log_set: String::new(),
            log_set_remote: false,
            remote_computers: Vec::new(),
            update_when: UpdateCheck::Start,
            update_check_old: None,
            update_last_check: 0,
            update_skipped: String::new(),
        }
    }
}

impl Config {
    /// Example rules (names in the current UI language).
    pub fn default_rules() -> Vec<HighlightRule> {
        vec![
            HighlightRule {
                name: t!("rule.default.success"),
                pattern: r"\bsuccess(fully)?\b|\bcompleted\b".into(),
                is_regex: true,
                scope: HighlightScope::Text,
                use_fg: true,
                fg: [0, 0, 0, 255],
                use_bg: true,
                bg: [130, 220, 130, 255],
                fg_dark: [225, 255, 225, 255],
                bg_dark: [35, 105, 45, 255],
                ..Default::default()
            },
            HighlightRule {
                name: t!("rule.default.error_codes"),
                pattern: r"0x[0-9A-Fa-f]{8}".into(),
                is_regex: true,
                scope: HighlightScope::Text,
                use_fg: true,
                fg: [255, 255, 255, 255],
                use_bg: true,
                bg: [60, 110, 200, 255],
                fg_dark: [235, 240, 255, 255],
                bg_dark: [45, 80, 160, 255],
                ..Default::default()
            },
            HighlightRule {
                name: t!("rule.default.exit_code"),
                pattern: r"exit code \d+".into(),
                is_regex: true,
                enabled: false,
                scope: HighlightScope::Row,
                use_fg: true,
                fg: [0, 0, 0, 255],
                use_bg: true,
                bg: [200, 170, 255, 255],
                fg_dark: [245, 235, 255, 255],
                bg_dark: [95, 60, 150, 255],
                ..Default::default()
            },
        ]
    }

    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("RusTrace").join("config.json"))
    }

    /// Loads the settings. The second value is `false` on first run (no settings file yet).
    pub fn load() -> (Self, bool) {
        let text = Self::path().and_then(|p| std::fs::read_to_string(p).ok());
        let existed = text.is_some();
        let cfg = text.and_then(|s| Self::parse(&s)).unwrap_or_default();
        (cfg, existed)
    }

    fn parse(text: &str) -> Option<Self> {
        let mut cfg: Config = serde_json::from_str(text).ok()?;
        cfg.fill_missing_dark();
        cfg.migrate_update_check();
        cfg.normalize_rule_sets();
        Some(cfg)
    }

    /// The settings file as it is now, `None` when it is missing or cannot be read.
    pub fn read_file() -> Option<Self> {
        Self::parse(&std::fs::read_to_string(Self::path()?).ok()?)
    }

    /// The settings as JSON – the form in which instances compare and merge them.
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    pub fn from_value(v: serde_json::Value) -> Option<Self> {
        let mut cfg: Config = serde_json::from_value(v).ok()?;
        cfg.fill_missing_dark();
        cfg.migrate_update_check();
        cfg.normalize_rule_sets();
        Some(cfg)
    }

    /// The old switch "check automatically (once a day)": on → at every start, off → never.
    pub fn migrate_update_check(&mut self) {
        if self.update_check_old.take() == Some(false) {
            self.update_when = UpdateCheck::Never;
        }
    }

    /// Writes the settings file through a temporary file, so another running instance never
    /// reads a half-written file.
    pub fn write_value(v: &serde_json::Value) -> std::io::Result<()> {
        let Some(p) = Self::path() else { return Ok(()) };
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(v).map_err(std::io::Error::other)?;
        let tmp = p.with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&tmp, &json)?;
        if std::fs::rename(&tmp, &p).is_err() {
            let _ = std::fs::remove_file(&tmp);
            std::fs::write(&p, json)?;
        }
        Ok(())
    }

    /// Adds dark colors to rules from older versions or imported files.
    pub fn fill_missing_dark(&mut self) {
        for r in self
            .rules
            .iter_mut()
            .chain(self.rule_sets.iter_mut().flat_map(|s| s.rules.iter_mut()))
        {
            r.fill_missing_dark();
        }
    }

    /// Rules saved before rule sets (#70) become the first set; a settings file always ends up
    /// with at least one set.
    pub fn normalize_rule_sets(&mut self) {
        let old = std::mem::take(&mut self.rules);
        if self.rule_sets.is_empty() {
            self.rule_sets
                .push(RuleSet::new(t!("rules.set.default"), old));
            self.rule_sets_migrated = true;
        }
    }

    /// The rules that apply now, in priority order (#70): those of the active sets, set after
    /// set. `def_ids` = ids of the log definitions of the open logs.
    pub fn active_rules(&self, def_ids: &[String]) -> Vec<&HighlightRule> {
        self.rule_sets
            .iter()
            .filter(|s| s.is_active(def_ids))
            .flat_map(|s| &s.rules)
            .collect()
    }

    /// Severity and search colors for the light or dark theme.
    pub fn palette(&self, dark: bool) -> Palette {
        if dark {
            Palette {
                error: (self.error_fg_dark, self.error_bg_dark),
                warning: (self.warning_fg_dark, self.warning_bg_dark),
                search: (self.search_fg_dark, self.search_bg_dark),
                bookmark: (self.bookmark_fg_dark, self.bookmark_bg_dark),
                gap: (self.gap_fg_dark, self.gap_bg_dark),
            }
        } else {
            Palette {
                error: (self.error_fg, self.error_bg),
                warning: (self.warning_fg, self.warning_bg),
                search: (self.search_fg, self.search_bg),
                bookmark: (self.bookmark_fg, self.bookmark_bg),
                gap: (self.gap_fg, self.gap_bg),
            }
        }
    }

    /// Editable palette colors of one theme: (label key, text, background), in the order of
    /// `Palette::pairs`.
    pub fn palette_mut(&mut self, dark: bool) -> [(&'static str, &mut [u8; 4], &mut [u8; 4]); 5] {
        if dark {
            [
                (
                    "severity.error",
                    &mut self.error_fg_dark,
                    &mut self.error_bg_dark,
                ),
                (
                    "severity.warning",
                    &mut self.warning_fg_dark,
                    &mut self.warning_bg_dark,
                ),
                (
                    "colors.search_match",
                    &mut self.search_fg_dark,
                    &mut self.search_bg_dark,
                ),
                (
                    "colors.bookmark",
                    &mut self.bookmark_fg_dark,
                    &mut self.bookmark_bg_dark,
                ),
                ("colors.gap", &mut self.gap_fg_dark, &mut self.gap_bg_dark),
            ]
        } else {
            [
                ("severity.error", &mut self.error_fg, &mut self.error_bg),
                (
                    "severity.warning",
                    &mut self.warning_fg,
                    &mut self.warning_bg,
                ),
                (
                    "colors.search_match",
                    &mut self.search_fg,
                    &mut self.search_bg,
                ),
                (
                    "colors.bookmark",
                    &mut self.bookmark_fg,
                    &mut self.bookmark_bg,
                ),
                ("colors.gap", &mut self.gap_fg, &mut self.gap_bg),
            ]
        }
    }

    /// Resets the palette colors of one theme to the defaults.
    pub fn reset_palette(&mut self, dark: bool) {
        self.set_palette(dark, &Config::default().palette(dark));
    }

    /// Sets the palette colors of one theme (reset, copy from the other theme).
    pub fn set_palette(&mut self, dark: bool, src: &Palette) {
        for ((_, fg, bg), (sfg, sbg)) in self.palette_mut(dark).into_iter().zip(src.pairs()) {
            *fg = sfg;
            *bg = sbg;
        }
    }

    pub fn interval_ms(&self) -> f64 {
        self.refresh_unit.to_ms(self.refresh_value).max(50.0)
    }

    pub fn push_recent(&mut self, path: &str) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_owned());
        self.recent_files.truncate(12);
    }

    /// Puts a search text at the top of the search history (no duplicates, at most
    /// `SEARCH_HISTORY_MAX` texts). Returns false when nothing changed.
    pub fn push_search(&mut self, text: &str) -> bool {
        if text.trim().is_empty() || self.search_history.first().is_some_and(|t| t == text) {
            return false;
        }
        self.search_history.retain(|t| t != text);
        self.search_history.insert(0, text.to_owned());
        self.search_history.truncate(SEARCH_HISTORY_MAX);
        true
    }

    /// Remembers a remote computer name (newest first, the same name in other case only once).
    pub fn push_computer(&mut self, name: &str) {
        self.remote_computers
            .retain(|c| !c.eq_ignore_ascii_case(name));
        self.remote_computers.insert(0, name.to_owned());
        self.remote_computers.truncate(REMOTE_COMPUTERS_MAX);
    }
}

/// Settings shared by several running instances: puts every top-level setting this instance
/// changed since `base` (the file as it was last read or written) into `disk` (the file now);
/// the other settings keep the value of `disk`, i.e. the changes of the other instances.
pub fn merge_settings(
    base: &serde_json::Value,
    mine: &serde_json::Value,
    disk: &mut serde_json::Value,
) {
    let (Some(mine), Some(disk)) = (mine.as_object(), disk.as_object_mut()) else {
        return;
    };
    for (key, value) in mine {
        if base.get(key) != Some(value) {
            disk.insert(key.clone(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_of_two_instances_are_merged() {
        let base = Config::default();
        let mut mine = base.clone();
        mine.font_size = 16.0;
        mine.push_search("mine");
        let mut other = base.clone();
        other.theme = "dark".into();
        other.font_size = 10.0;
        other.rule_sets[0].rules.clear();
        let mut disk = other.to_value();
        merge_settings(&base.to_value(), &mine.to_value(), &mut disk);
        let merged = Config::from_value(disk).unwrap();
        // Own changes win for the settings this instance changed, the others come from the file.
        assert_eq!(merged.font_size, 16.0);
        assert_eq!(merged.search_history, ["mine"]);
        assert_eq!(merged.theme, "dark");
        assert!(merged.rule_sets[0].rules.is_empty());

        // Nothing changed here: the file is taken as it is.
        let mut disk = other.to_value();
        merge_settings(&base.to_value(), &base.to_value(), &mut disk);
        assert_eq!(disk, other.to_value());
    }

    #[test]
    fn importing_rules_twice_creates_no_duplicates() {
        // The shared rule file of the guide client-msi-1603 (#51).
        let file = include_str!("../docs/guides/rules/msi-verbose-log.json");
        let parse = || serde_json::from_str::<Vec<HighlightRule>>(file).unwrap();
        let mut rules = Config::default_rules();
        let base = rules.len();
        let first = import_rules(&mut rules, parse());
        assert_eq!(
            first,
            RuleImport {
                added: 3,
                updated: 0,
                skipped: 0
            }
        );
        let second = import_rules(&mut rules, parse());
        assert_eq!(
            second,
            RuleImport {
                added: 0,
                updated: 0,
                skipped: 3
            }
        );
        assert_eq!(rules.len(), base + 3);

        // A newer version of the file with other colors updates the rule in place and keeps
        // whether the user turned it off; a new rule is appended.
        rules[base].enabled = false;
        let mut newer = parse();
        newer[0].bg = [1, 2, 3, 255];
        newer[0].bg_dark = UNSET; // older file without dark colors: filled from the light ones
        newer.push(HighlightRule {
            name: "MSI: other".into(),
            pattern: "x".into(),
            ..HighlightRule::default()
        });
        let third = import_rules(&mut rules, newer);
        assert_eq!(
            third,
            RuleImport {
                added: 1,
                updated: 1,
                skipped: 2
            }
        );
        assert_eq!(rules.len(), base + 4);
        assert_eq!(rules[base].bg, [1, 2, 3, 255]);
        assert_eq!(rules[base].bg_dark, [1, 2, 3, 255]);
        assert!(!rules[base].enabled);
        assert_eq!(rules[base + 3].name, "MSI: other");

        // Same name, other pattern = another rule.
        let mut other = parse();
        other.truncate(1);
        other[0].pattern.push('x');
        assert_eq!(import_rules(&mut rules, other).added, 1);
    }

    #[test]
    fn remote_computers_newest_first() {
        let mut cfg = Config::default();
        for i in 0..20 {
            cfg.push_computer(&format!("pc{i}"));
        }
        cfg.push_computer("PC5");
        assert_eq!(cfg.remote_computers.len(), REMOTE_COMPUTERS_MAX);
        assert_eq!(cfg.remote_computers[0], "PC5");
        assert_eq!(cfg.remote_computers[1], "pc19");
        assert!(!cfg.remote_computers[1..].iter().any(|c| c == "pc5"));
        // Old settings files without the new keys still load.
        let old: Config = serde_json::from_str("{}").unwrap();
        assert!(!old.log_set_remote && old.remote_computers.is_empty());
    }

    /// Settings from an older version (no dark colors) get the light colors for the dark theme.
    #[test]
    fn old_rules_get_dark_colors() {
        let json = r#"{"rules":[{"pattern":"x","fg":[1,2,3,255],"bg":[4,5,6,255]}]}"#;
        let cfg = Config::parse(json).unwrap();
        assert_eq!(
            cfg.rule_sets[0].rules[0].colors(true),
            ([1, 2, 3, 255], [4, 5, 6, 255])
        );
        // Missing severity colors come from the defaults.
        assert_eq!(cfg.error_bg_dark, Config::default().error_bg_dark);
    }

    /// Rules of a version without rule sets become the first set - and only those rules (#70).
    #[test]
    fn old_rules_become_the_first_set() {
        let cfg = Config::parse(r#"{"rules":[{"name":"mine","pattern":"x"}]}"#).unwrap();
        assert!(cfg.rules.is_empty());
        assert_eq!(cfg.rule_sets.len(), 1);
        assert_eq!(cfg.rule_sets[0].rules.len(), 1);
        assert_eq!(cfg.rule_sets[0].rules[0].name, "mine");
        assert!(cfg.rule_sets[0].enabled && cfg.rule_sets[0].logdefs.is_empty());
        // A file without any rules does not get the examples.
        let none = Config::parse("{}").unwrap();
        assert_eq!(none.rule_sets.len(), 1);
        assert!(none.rule_sets[0].rules.is_empty());
        // Saved again: the legacy key is gone, and the sets survive a round trip unchanged.
        let json = serde_json::to_string(&cfg).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value.get("rules").is_none());
        let back = Config::parse(&json).unwrap();
        assert_eq!(back.rule_sets, cfg.rule_sets);
        // A fresh configuration has the example set.
        let fresh = Config::default();
        assert_eq!(fresh.rule_sets.len(), 1);
        assert_eq!(fresh.rule_sets[0].rules, Config::default_rules());
    }

    #[test]
    fn rule_sets_are_active_by_switch_and_binding() {
        let rule = |n: &str| HighlightRule {
            name: n.into(),
            ..HighlightRule::default()
        };
        let set = |n: &str, on: bool, defs: &[&str]| RuleSet {
            name: n.into(),
            enabled: on,
            logdefs: defs.iter().map(|d| d.to_string()).collect(),
            rules: vec![rule(n)],
        };
        let cfg = Config {
            rule_sets: vec![
                set("all", true, &[]),
                set("off", false, &[]),
                set("cbs", true, &["cbs"]),
                set("iis", true, &["IIS", "http"]),
            ],
            ..Config::default()
        };
        let names = |ids: &[&str]| -> Vec<String> {
            let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
            cfg.active_rules(&ids)
                .iter()
                .map(|r| r.name.clone())
                .collect()
        };
        assert_eq!(names(&[]), ["all"]);
        assert_eq!(names(&["cbs"]), ["all", "cbs"]);
        // Ids are compared without regard to case, any of several logs counts.
        assert_eq!(names(&["CBS", "iis"]), ["all", "cbs", "iis"]);
        assert_eq!(names(&["other"]), ["all"]);
    }

    #[test]
    fn search_history() {
        let mut cfg = Config::default();
        assert!(!cfg.push_search("  "));
        assert!(cfg.push_search("error"));
        assert!(cfg.push_search("0x8007"));
        assert!(!cfg.push_search("0x8007"));
        // A used text moves to the top instead of being added twice.
        assert!(cfg.push_search("error"));
        assert_eq!(cfg.search_history, ["error", "0x8007"]);
        for i in 0..100 {
            cfg.push_search(&format!("text {i}"));
        }
        assert_eq!(cfg.search_history.len(), SEARCH_HISTORY_MAX);
        assert_eq!(cfg.search_history[0], "text 99");
        // Old settings files without the history still load.
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.search_history.is_empty());
    }

    #[test]
    fn update_check_migration() {
        let load = |json: &str| {
            let mut cfg: Config = serde_json::from_str(json).unwrap();
            cfg.migrate_update_check();
            cfg
        };
        assert_eq!(
            load(r#"{"update_check":true}"#).update_when,
            UpdateCheck::Start
        );
        assert_eq!(
            load(r#"{"update_check":false}"#).update_when,
            UpdateCheck::Never
        );
        assert_eq!(load("{}").update_when, UpdateCheck::Start);
        let cfg = load(r#"{"update_when":"daily"}"#);
        assert_eq!(cfg.update_when, UpdateCheck::Daily);
        // Saved without the old switch, and read back the same.
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains(r#""update_when":"daily""#) && !json.contains("update_check\""));
        assert_eq!(load(&json).update_when, UpdateCheck::Daily);

        assert!(UpdateCheck::Start.due(1000, 1001));
        assert!(!UpdateCheck::Daily.due(1000, 1000 + 24 * 3600 - 1));
        assert!(UpdateCheck::Daily.due(1000, 1000 + 24 * 3600));
        assert!(!UpdateCheck::Never.due(0, i64::MAX));
    }

    #[test]
    fn palette_reset_and_copy() {
        let mut cfg = Config {
            error_bg_dark: [9, 9, 9, 255],
            ..Config::default()
        };
        cfg.reset_palette(true);
        assert_eq!(cfg.error_bg_dark, Config::default().error_bg_dark);
        let mut r = HighlightRule::default();
        r.copy_colors(false);
        assert_eq!(r.colors(true), r.colors(false));
    }
}
