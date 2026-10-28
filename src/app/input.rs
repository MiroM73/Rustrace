//! Keyboard shortcuts and drag & drop.
//!
//! Note on shortcut order (#46): egui's `consume_key` matches modifiers with
//! `Modifiers::matches_logically`, which ignores *extra* Shift and Alt. A pattern without Shift
//! therefore also consumes the Shift variant of the same key, so the more specific shortcut
//! (`SHIFT`, `COMMAND | SHIFT`) must always be consumed first. Ctrl / Cmd is matched exactly,
//! so `COMMAND` vs `NONE` pairs (F5 / Ctrl+F5, F2 / Ctrl+F2) do not depend on the order.

use crate::app::*;

/// Consumes `key` and `Shift+key` and returns `(plain, shifted)`.
///
/// `Shift+key` is checked first: `consume_key(Modifiers::NONE, key)` would otherwise swallow it
/// too (see the module note, #46).
fn consume_key_and_shifted(i: &mut egui::InputState, key: Key) -> (bool, bool) {
    let shifted = i.consume_key(Modifiers::SHIFT, key);
    let plain = i.consume_key(Modifiers::NONE, key);
    (plain, shifted)
}

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
        let (open, f5, ctrl_f5, ctrl_f, (f3, sh_f3), pause, ctrl_h, ctrl_l, ctrl_e) = ctx
            .input_mut(|i| {
                (
                    i.consume_key(Modifiers::COMMAND, Key::O),
                    i.consume_key(Modifiers::NONE, Key::F5),
                    i.consume_key(Modifiers::COMMAND, Key::F5),
                    i.consume_key(Modifiers::COMMAND, Key::F),
                    consume_key_and_shifted(i, Key::F3),
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
        let (ctrl_f2, (f2, sh_f2)) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::F2),
                consume_key_and_shifted(i, Key::F2),
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
        let (up, down, pgup, pgdn, home, end, copy, shift) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::PageUp),
                i.key_pressed(Key::PageDown),
                i.key_pressed(Key::Home),
                i.key_pressed(Key::End),
                i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                i.modifiers.shift,
            )
        });
        // Several rows (#25): Ctrl+A = all shown rows, Ctrl+Shift+C = copy as a table.
        let (all, copy_table) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::A),
                i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::C),
            )
        });
        if all {
            self.select_all_visible();
        }
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
            // Shift + arrows / PgUp / PgDn / Home / End extend the selection from the anchor.
            if shift {
                self.extend_selection(t, false);
            } else {
                self.select_row(t);
            }
        }
        if self.selected.is_some() {
            if copy {
                self.copy_messages(ctx);
            }
            if copy_table {
                self.copy_table(ctx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_with_key(key: Key, modifiers: Modifiers) -> egui::InputState {
        let mut i = egui::InputState::default();
        i.modifiers = modifiers;
        i.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
        i
    }

    #[test]
    fn shift_f3_and_shift_f2_are_not_taken_by_the_plain_shortcut() {
        for key in [Key::F3, Key::F2] {
            let mut i = input_with_key(key, Modifiers::SHIFT);
            assert_eq!(
                consume_key_and_shifted(&mut i, key),
                (false, true),
                "Shift+{key:?}"
            );
            let mut i = input_with_key(key, Modifiers::NONE);
            assert_eq!(
                consume_key_and_shifted(&mut i, key),
                (true, false),
                "{key:?}"
            );
            // Ctrl+key is a different shortcut (Ctrl+F2 = toggle bookmark) and stays untouched.
            let mut i = input_with_key(key, Modifiers::COMMAND);
            assert_eq!(
                consume_key_and_shifted(&mut i, key),
                (false, false),
                "Ctrl+{key:?}"
            );
        }
    }
}
