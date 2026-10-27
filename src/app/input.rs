//! Keyboard shortcuts and drag & drop.

use crate::app::*;

impl App {
    pub(in crate::app) fn handle_input(&mut self, ctx: &egui::Context) {
        // Drag & drop of files onto the window (several files: one view merged by time).
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            self.open_files(dropped);
        }

        let typing = ctx.egui_wants_keyboard_input();
        let export_key =
            ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S));
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
        // Bookmarks (#22): Ctrl+F2 toggles the selected row, F2 / Shift+F2 = next / previous.
        let (ctrl_f2, f2, sh_f2) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::F2),
                i.consume_key(Modifiers::NONE, Key::F2),
                i.consume_key(Modifiers::SHIFT, Key::F2),
            )
        });
        if ctrl_f2 && let Some(i) = self.selected {
            self.toggle_bookmark(i);
        }
        if f2 || sh_f2 {
            self.goto_bookmark(f2);
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
}
