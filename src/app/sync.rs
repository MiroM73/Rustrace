//! Settings shared by running instances: a change saved by one instance is taken over by the
//! others (they watch the settings file).

use crate::app::*;
use crate::config::merge_settings;

/// How often the settings file is checked for changes of another instance.
const WATCH_INTERVAL: Duration = Duration::from_secs(1);

/// Watches the settings file in a background thread; the flag is set (and a frame requested)
/// whenever the file changes.
pub(in crate::app) fn watch_settings_file(ctx: egui::Context) -> Arc<AtomicBool> {
    let changed = Arc::new(AtomicBool::new(false));
    let Some(path) = Config::path() else {
        return changed;
    };
    let flag = changed.clone();
    let stamp = move || {
        std::fs::metadata(&path)
            .ok()
            .map(|m| (m.modified().ok(), m.len()))
    };
    let _ = std::thread::Builder::new()
        .name("settings-watch".into())
        .spawn(move || {
            let mut last = stamp();
            loop {
                std::thread::sleep(WATCH_INTERVAL);
                // The app is gone.
                if Arc::strong_count(&flag) == 1 {
                    break;
                }
                let now = stamp();
                if now != last {
                    last = now;
                    flag.store(true, Ordering::Relaxed);
                    ctx.request_repaint();
                }
            }
        });
    changed
}

impl App {
    /// Saves the settings; settings another instance saved meanwhile are kept and taken over.
    pub(in crate::app) fn save_settings(&mut self, ctx: &egui::Context) -> std::io::Result<()> {
        let mine = self.cfg.to_value();
        let mut merged = mine.clone();
        if let Some(disk) = Config::read_file().map(|c| c.to_value())
            && disk != self.cfg_base
        {
            merged = disk;
            merge_settings(&self.cfg_base, &mine, &mut merged);
        }
        Config::write_value(&merged)?;
        self.cfg_base = merged.clone();
        if merged != mine {
            self.apply_shared_settings(ctx, merged);
        }
        Ok(())
    }

    /// Takes over the settings another instance has saved (own unsaved changes stay).
    pub(in crate::app) fn check_shared_settings(&mut self, ctx: &egui::Context) {
        if !self.cfg_changed.swap(false, Ordering::Relaxed) {
            return;
        }
        let Some(disk) = Config::read_file().map(|c| c.to_value()) else {
            return;
        };
        if disk == self.cfg_base {
            return;
        }
        let mut merged = disk.clone();
        merge_settings(&self.cfg_base, &self.cfg.to_value(), &mut merged);
        self.cfg_base = disk;
        self.apply_shared_settings(ctx, merged);
    }

    /// Uses new settings and applies what does not take effect by itself in the next frame.
    fn apply_shared_settings(&mut self, ctx: &egui::Context, v: serde_json::Value) {
        let Some(new) = Config::from_value(v) else {
            return;
        };
        let old = std::mem::replace(&mut self.cfg, new);
        let dirty = self.cfg_dirty;
        if old.theme != self.cfg.theme {
            self.apply_theme(ctx);
        }
        if old.ui_zoom != self.cfg.ui_zoom {
            ctx.set_zoom_factor(
                self.cfg
                    .ui_zoom
                    .clamp(*UI_ZOOM_RANGE.start(), *UI_ZOOM_RANGE.end()),
            );
        }
        if old.language != self.cfg.language {
            let code = self.cfg.language.clone();
            self.set_language(&code, false);
        }
        if old.rules != self.cfg.rules {
            self.recompile_rules();
        }
        if old.overview != self.cfg.overview {
            self.overview_changed_from(0);
        }
        if old.auto_scroll != self.cfg.auto_scroll {
            self.follow.resume();
            if self.cfg.auto_scroll {
                self.scroll_to_end_after_filter();
            }
        }
        if (old.max_log_enabled, old.max_log_mb) != (self.cfg.max_log_enabled, self.cfg.max_log_mb)
        {
            self.enforce_memory_limits();
        }
        let defs_changed = old.logdef_sources != self.cfg.logdef_sources;
        if defs_changed {
            self.load_defs();
        }
        let reread = defs_changed
            || old.keyword_severity != self.cfg.keyword_severity
            || old.merge_rotated != self.cfg.merge_rotated;
        if reread && self.cur_path.is_some() {
            self.reload_full();
        }
        self.cfg_dirty = dirty;
        ctx.request_repaint();
    }
}
