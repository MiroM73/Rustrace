//! Application settings – stored as JSON in the user's configuration folder
//! (Windows: %APPDATA%\RusTrace\config.json).

use crate::export::ExportFormat;
use crate::t;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

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

/// Severity and search colors of one theme: (text, background).
pub struct Palette {
    pub error: ([u8; 4], [u8; 4]),
    pub warning: ([u8; 4], [u8; 4]),
    pub search: ([u8; 4], [u8; 4]),
}

/// How many search texts the search history keeps.
pub const SEARCH_HISTORY_MAX: usize = 30;

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
    pub recent_files: Vec<String>,
    /// Previously used search texts, newest first.
    pub search_history: Vec<String>,
    pub show_details: bool,
    /// Show the filter row under the column headers.
    pub show_column_filters: bool,
    /// Show the time range (from – to) filter row.
    pub show_time_range: bool,
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

    /// Keep the message text of files of at least `temp_file_threshold_mb` MB in a memory-mapped
    /// temporary file instead of the process memory.
    pub temp_file_enabled: bool,
    pub temp_file_threshold_mb: u32,
    /// Also read the rotated older parts of a log (`name.lo_`, `name-YYYYMMDD-HHMMSS.log`) and keep
    /// the history when the log is rotated while it is watched.
    pub merge_rotated: bool,
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
            recent_files: Vec::new(),
            search_history: Vec::new(),
            show_details: true,
            show_column_filters: false,
            show_time_range: false,
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
            export_format: ExportFormat::Original,
            export_separators: true,
            export_since_enabled: false,
            export_since: String::new(),
            export_last_enabled: false,
            export_last_blocks: 10,
            export_visible_only: false,
            temp_file_enabled: true,
            temp_file_threshold_mb: 100,
            merge_rotated: true,
            max_log_enabled: true,
            max_log_mb: 1000,
            logdef_sources: "all".into(),
            known_logdefs: BTreeMap::new(),
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
        (cfg, existed)
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
            }
        } else {
            Palette {
                error: (self.error_fg, self.error_bg),
                warning: (self.warning_fg, self.warning_bg),
                search: (self.search_fg, self.search_bg),
            }
        }
    }

    /// Editable severity/search colors of one theme: (label key, text, background).
    pub fn palette_mut(&mut self, dark: bool) -> [(&'static str, &mut [u8; 4], &mut [u8; 4]); 3] {
        if dark {
            [
                ("severity.error", &mut self.error_fg_dark, &mut self.error_bg_dark),
                ("severity.warning", &mut self.warning_fg_dark, &mut self.warning_bg_dark),
                ("colors.search_match", &mut self.search_fg_dark, &mut self.search_bg_dark),
            ]
        } else {
            [
                ("severity.error", &mut self.error_fg, &mut self.error_bg),
                ("severity.warning", &mut self.warning_fg, &mut self.warning_bg),
                ("colors.search_match", &mut self.search_fg, &mut self.search_bg),
            ]
        }
    }

    /// Resets the severity/search colors of one theme to the defaults.
    pub fn reset_palette(&mut self, dark: bool) {
        let d = Config::default();
        let src = d.palette(dark);
        let [e, w, s] = self.palette_mut(dark);
        for ((_, fg, bg), (dfg, dbg)) in [e, w, s].into_iter().zip([src.error, src.warning, src.search]) {
            *fg = dfg;
            *bg = dbg;
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn palette_reset_and_copy() {
        let mut cfg = Config::default();
        cfg.error_bg_dark = [9, 9, 9, 255];
        cfg.reset_palette(true);
        assert_eq!(cfg.error_bg_dark, Config::default().error_bg_dark);
        let mut r = HighlightRule::default();
        r.copy_colors(false);
        assert_eq!(r.colors(true), r.colors(false));
    }
}
