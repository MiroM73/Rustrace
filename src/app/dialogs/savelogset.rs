//! File → Save as log set (#76): the logs of the merged view become an own log set – a name, a
//! description and the `file` lines (with variables where possible), written to the user's sets
//! folder. A set of that name is overwritten only on request.

use crate::app::*;
use crate::logset::{self, LogSetDef};
use crate::logsetsave::{self, Changes, Collision};

/// State of the open "Save as log set" window.
pub(in crate::app) struct SaveSetDialog {
    name: String,
    description: String,
    /// The own set the fields were filled from and its description, so an unchanged field does
    /// not overwrite the other translations.
    prefilled: Option<(PathBuf, String)>,
    /// The opened logs when the window was opened, and the `file` lines made of them.
    paths: Vec<PathBuf>,
    lines: Vec<String>,
    /// The loaded sets and the file names of the user's folder, to see collisions.
    sets: Vec<LogSetDef>,
    files: Vec<String>,
    dir: Option<PathBuf>,
    error: Option<String>,
    focus: bool,
}

impl SaveSetDialog {
    fn collision(&self) -> Option<Collision> {
        logsetsave::collision(
            &self.sets,
            self.dir.as_deref()?,
            &self.files,
            self.name.trim(),
        )
    }

    /// Reads the sets and the folder again.
    fn rescan(&mut self) {
        self.sets = logset::load_all().sets;
        self.files = self
            .dir
            .as_deref()
            .map(logsetsave::files_in)
            .unwrap_or_default();
    }
}

impl App {
    /// File → Save as log set: for the logs opened now.
    pub(in crate::app) fn open_save_log_set(&mut self) {
        let absolute: Vec<String> = self
            .paths
            .iter()
            .map(|p| {
                std::path::absolute(p)
                    .unwrap_or_else(|_| p.clone())
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        let mut dlg = SaveSetDialog {
            name: String::new(),
            description: String::new(),
            prefilled: None,
            paths: self.paths.clone(),
            lines: logsetsave::set_lines(&absolute, &logset::this_computer_vars()),
            sets: Vec::new(),
            files: Vec::new(),
            dir: logset::user_set_dir(),
            error: None,
            focus: true,
        };
        dlg.rescan();
        // Opened from a set: offer to save it again under the same name (own sets first).
        if let Some(name) = &self.set_name {
            dlg.name = name.clone();
            let from = dlg
                .sets
                .iter()
                .filter(|s| s.name(&self.lang_code) == *name)
                .max_by_key(|s| s.is_own());
            if let Some(s) = from {
                dlg.description = s.description(&self.lang_code).unwrap_or_default().into();
                if let Some(path) = s.path.clone().filter(|_| s.is_own()) {
                    dlg.prefilled = Some((path, dlg.description.clone()));
                }
            }
        }
        self.save_set = Some(dlg);
        self.save_set_win.open();
    }

    /// Writes the set; returns whether it is saved. `overwrite` = the user asked to replace
    /// what the window showed as existing.
    fn save_log_set(&mut self, dlg: &mut SaveSetDialog, overwrite: bool) -> bool {
        dlg.error = None;
        let shown = dlg.collision();
        dlg.rescan();
        let collision = dlg.collision();
        // The folder changed in the meantime: show it before anything is replaced.
        if collision.is_some() && (!overwrite || collision != shown) {
            return false;
        }
        let Some(dir) = dlg.dir.clone() else {
            return false;
        };
        let name = dlg.name.trim().to_owned();
        let keep = collision.as_ref().is_some_and(|c| c.same_name);
        let path = collision.map_or_else(|| dir.join(logsetsave::file_name(&name)), |c| c.path);
        let description = dlg.description.trim();
        // An empty field removes the description only when it showed this set's one.
        let description = match &dlg.prefilled {
            Some((from, old)) if keep && *from == path => {
                (description != old).then_some(description)
            }
            _ => (!description.is_empty()).then_some(description),
        };
        let changes = Changes {
            name: Some(&name),
            description,
            files: Some(&dlg.lines),
        };
        if let Err(e) = logsetsave::save(&path, keep, &self.lang_code, &changes) {
            dlg.error = Some(t!("logset.save.error", file = path.display(), error = e));
            return false;
        }
        if let Some(id) = path.file_name() {
            self.cfg.last_log_set = format!("own/{}", id.to_string_lossy().to_lowercase());
            self.cfg_dirty = true;
        }
        // The view is this set now (unless other logs were opened meanwhile).
        if self.paths == dlg.paths {
            self.set_name = Some(name);
        }
        self.info(t!("logset.save.done", file = path.display()));
        self.reload_log_sets();
        true
    }

    pub(in crate::app) fn save_log_set_window(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.save_set.take() else {
            self.save_set_win.close();
            return;
        };
        let (mut save, mut overwrite, mut close) = (false, false, false);
        let spec = ToolSpec::new(
            format!("💾 {}", t!("logset.save.title")),
            [720.0, 440.0],
            [480.0, 300.0],
        );
        let open = self.show_tool(
            ctx,
            |a| &mut a.save_set_win,
            spec,
            |_app, ui, _keep| {
                let mut enter = false;
                egui::Grid::new("save_logset_fields")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(t!("logset.save.name"));
                        let r =
                            ui.add(egui::TextEdit::singleline(&mut dlg.name).desired_width(420.0));
                        if dlg.focus {
                            r.request_focus();
                            dlg.focus = false;
                        }
                        enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        ui.end_row();
                        ui.label(t!("logset.save.description"));
                        ui.add(
                            egui::TextEdit::multiline(&mut dlg.description)
                                .hint_text(t!("logset.save.description.hint"))
                                .desired_rows(2)
                                .desired_width(420.0),
                        );
                        ui.end_row();
                    });
                ui.separator();
                ui.label(t!("logset.save.files", count = dlg.lines.len()));
                egui::ScrollArea::vertical()
                    .id_salt("save_logset_files")
                    .max_height(220.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for line in &dlg.lines {
                            ui.add(egui::Label::new(RichText::new(line).monospace()).extend());
                        }
                    });
                ui.separator();
                if let Some(dir) = &dlg.dir {
                    ui.weak(t!("logset.save.folder", path = dir.display()));
                }
                let collision = dlg.collision();
                if let Some(c) = &collision {
                    ui.colored_label(ORANGE, t!("logset.save.exists", file = c.path.display()));
                }
                if let Some(e) = &dlg.error {
                    ui.colored_label(RED, e);
                }
                let ok = !dlg.name.trim().is_empty() && dlg.dir.is_some();
                ui.horizontal(|ui| {
                    let text = if collision.is_some() {
                        format!("💾 {}", t!("logset.save.overwrite"))
                    } else {
                        format!("💾 {}", t!("logset.save.button"))
                    };
                    if ui.add_enabled(ok, egui::Button::new(text)).clicked() {
                        save = true;
                        overwrite = collision.is_some();
                    }
                    if ui.button(t!("progress.cancel")).clicked() {
                        close = true;
                    }
                });
                // Enter never overwrites.
                save |= ok && enter && collision.is_none();
            },
        );
        if save && self.save_log_set(&mut dlg, overwrite) {
            close = true;
        }
        if open && !close {
            self.save_set = Some(dlg);
        } else {
            self.save_set_win.close();
        }
    }
}
