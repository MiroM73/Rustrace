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

#[derive(Clone, Serialize, Deserialize, Debug)]
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
    fn fill_missing_dark(&mut self) {
        if self.fg_dark == UNSET {
            self.fg_dark = self.fg;
        }
        if self.bg_dark == UNSET {
            self.bg_dark = self.bg;
        }
    }
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

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Config {
    pub rules: Vec<HighlightRule>,
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
            rules: Self::default_rules(),
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
        let mut cfg: Config = text
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        cfg.fill_missing_dark();
        cfg.migrate_update_check();
        (cfg, existed)
    }

    /// The old switch "check automatically (once a day)": on → at every start, off → never.
    pub fn migrate_update_check(&mut self) {
        if self.update_check_old.take() == Some(false) {
            self.update_when = UpdateCheck::Never;
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(p) = Self::path() else { return Ok(()) };
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(p, json)
    }

    /// Adds dark colors to rules from older versions or imported files.
    pub fn fill_missing_dark(&mut self) {
        for r in &mut self.rules {
            r.fill_missing_dark();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut cfg: Config = serde_json::from_str(json).unwrap();
        cfg.fill_missing_dark();
        assert_eq!(cfg.rules[0].colors(true), ([1, 2, 3, 255], [4, 5, 6, 255]));
        // Missing severity colors come from the defaults.
        assert_eq!(cfg.error_bg_dark, Config::default().error_bg_dark);
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
