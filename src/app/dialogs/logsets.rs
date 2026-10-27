//! File → Open log set (#27): a predefined group of logs for a troubleshooting scenario, opened
//! as one view merged by time. Which files exist is checked in a background thread.

use crate::app::*;
use crate::logset::{self, FileState, LogSetDef};

/// State of the open "Open log set" window.
pub(in crate::app) struct LogSetDialog {
    sets: Vec<LogSetDef>,
    errors: Vec<String>,
    chosen: usize,
    rows: Vec<SetRow>,
    /// Results of the running check (one per row with a resolved path, in row order).
    check: Option<mpsc::Receiver<Vec<FileState>>>,
}

/// One log of the chosen set.
struct SetRow {
    /// The `file` line as written in the set.
    file: String,
    /// The resolved path, or the unknown variable.
    path: Result<PathBuf, String>,
    /// `None` = not checked yet.
    state: Option<FileState>,
    ticked: bool,
}

impl LogSetDialog {
    fn set(&self) -> Option<&LogSetDef> {
        self.sets.get(self.chosen)
    }
}

/// The variables of this computer (`%CCMLOGS%` from the registry of the ConfigMgr client).
fn vars() -> logset::Vars {
    #[cfg(windows)]
    let ccm =
        crate::fileassoc::hklm_string(r"SOFTWARE\Microsoft\CCM\Logging\@Global", "LogDirectory");
    #[cfg(not(windows))]
    let ccm = None;
    logset::local_vars(ccm)
}

impl App {
    /// File → Open log set: (re)loads the set files and checks the last chosen set.
    pub(in crate::app) fn open_log_sets(&mut self) {
        let list = logset::load_all();
        let mut sets = list.sets;
        // Predefined first, then own; each group by its name in the current language.
        let lang = self.lang_code.clone();
        sets.sort_by_cached_key(|s| (s.is_own(), s.name(&lang).to_lowercase()));
        let chosen = sets
            .iter()
            .position(|s| s.key() == self.cfg.last_log_set)
            .unwrap_or(0);
        let mut dlg = LogSetDialog {
            sets,
            errors: list.errors,
            chosen,
            rows: Vec::new(),
            check: None,
        };
        self.start_set_check(&mut dlg);
        self.log_sets = Some(dlg);
    }

    /// Resolves the files of the chosen set and checks them in a background thread.
    fn start_set_check(&self, dlg: &mut LogSetDialog) {
        let vars = vars();
        dlg.rows = dlg.set().map_or_else(Vec::new, |s| {
            s.files
                .iter()
                .map(|f| SetRow {
                    file: f.clone(),
                    path: logset::resolve(f, &vars).map(PathBuf::from),
                    state: None,
                    ticked: false,
                })
                .collect()
        });
        let paths: Vec<PathBuf> = dlg
            .rows
            .iter()
            .filter_map(|r| r.path.clone().ok())
            .collect();
        let rotated = self.cfg.merge_rotated;
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("rustrace-logset".into())
            .spawn(move || {
                let states = paths
                    .iter()
                    .map(|p| logset::check_file(p, rotated))
                    .collect();
                let _ = tx.send(states);
                ctx.request_repaint();
            });
        dlg.check = spawned.is_ok().then_some(rx);
    }

    pub(in crate::app) fn log_sets_window(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.log_sets.take() else {
            return;
        };
        if let Some(rx) = &dlg.check
            && let Ok(states) = rx.try_recv()
        {
            let mut states = states.into_iter();
            for row in dlg.rows.iter_mut().filter(|r| r.path.is_ok()) {
                row.state = states.next();
                row.ticked = row.state.as_ref().is_some_and(FileState::exists);
            }
            dlg.check = None;
        }
        let mut open = true;
        let (mut chosen, mut recheck, mut open_logs, mut close, mut own_folder) =
            (None, false, false, false, false);
        let lang = self.lang_code.clone();
        egui::Window::new(format!("📑 {}", t!("logset.title")))
            .id(egui::Id::unique("logset_window"))
            .open(&mut open)
            .default_width(820.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t!("logset.set"));
                    let label = |s: &LogSetDef| {
                        if s.is_own() {
                            format!("{} ({})", s.name(&lang), t!("logset.own"))
                        } else {
                            s.name(&lang)
                        }
                    };
                    egui::ComboBox::from_id_salt("logset_combo")
                        .width(320.0)
                        .selected_text(dlg.set().map(label).unwrap_or_default())
                        .show_ui(ui, |ui| {
                            for (i, s) in dlg.sets.iter().enumerate() {
                                if ui.selectable_label(i == dlg.chosen, label(s)).clicked() {
                                    chosen = Some(i);
                                }
                            }
                        });
                });
                if let Some(s) = dlg.set() {
                    if let Some(d) = s.description(&lang) {
                        ui.add(egui::Label::new(RichText::new(d).weak()).wrap());
                    }
                    if let Some(p) = &s.path {
                        ui.weak(t!("logset.source", path = p.display()));
                    }
                }
                ui.separator();
                self.set_rows(ui, &mut dlg);
                ui.separator();
                let count = dlg
                    .rows
                    .iter()
                    .filter(|r| r.ticked && r.state.as_ref().is_some_and(FileState::exists))
                    .count();
                if dlg.check.is_none()
                    && !dlg.rows.is_empty()
                    && !dlg
                        .rows
                        .iter()
                        .any(|r| r.state.as_ref().is_some_and(FileState::exists))
                {
                    ui.colored_label(ORANGE, t!("logset.none_found"));
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            count > 0,
                            egui::Button::new(format!("📂 {}", t!("logset.open", count = count))),
                        )
                        .on_hover_text(t!("logset.open.tip"))
                        .clicked()
                    {
                        open_logs = true;
                    }
                    if ui
                        .add_enabled(
                            dlg.check.is_none(),
                            egui::Button::new(format!("🔄 {}", t!("logset.recheck"))),
                        )
                        .clicked()
                    {
                        recheck = true;
                    }
                    if ui
                        .button(format!("📁 {}", t!("logset.own_folder")))
                        .on_hover_text(t!("logset.own_folder.tip"))
                        .clicked()
                    {
                        own_folder = true;
                    }
                    if ui.button(t!("progress.cancel")).clicked() {
                        close = true;
                    }
                });
                if !dlg.errors.is_empty() {
                    ui.separator();
                    ui.colored_label(RED, t!("logset.errors"));
                    for e in &dlg.errors {
                        ui.label(RichText::new(e).small().color(RED));
                    }
                }
            });

        if own_folder && let Some(dir) = logset::user_set_dir() {
            let res = std::fs::create_dir_all(&dir).and_then(|_| open::that_detached(&dir));
            if let Err(e) = res {
                self.status_error = Some(t!("error.open_folder", error = e));
            }
        }
        if let Some(i) = chosen.filter(|&i| i != dlg.chosen) {
            dlg.chosen = i;
            if let Some(s) = dlg.set() {
                self.cfg.last_log_set = s.key();
                self.cfg_dirty = true;
            }
            recheck = true;
        }
        if recheck {
            self.start_set_check(&mut dlg);
        }
        if open_logs {
            let paths: Vec<PathBuf> = dlg
                .rows
                .iter()
                .filter(|r| r.ticked && r.state.as_ref().is_some_and(FileState::exists))
                .filter_map(|r| r.path.clone().ok())
                .collect();
            if let Some(s) = dlg.set() {
                self.cfg.last_log_set = s.key();
                self.cfg_dirty = true;
            }
            self.open_files(paths);
            return;
        }
        if open && !close {
            self.log_sets = Some(dlg);
        }
    }

    /// The table of the logs of the set: tick, state, path, size, last write time.
    fn set_rows(&self, ui: &mut egui::Ui, dlg: &mut LogSetDialog) {
        let green = if self.dark {
            Color32::from_rgb(110, 200, 110)
        } else {
            Color32::from_rgb(20, 130, 40)
        };
        let no_wrap = |ui: &mut egui::Ui, text: RichText| ui.add(egui::Label::new(text).extend());
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("logset_grid")
                    .striped(true)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("");
                        ui.strong(t!("logset.col.state"));
                        ui.strong(t!("logset.col.size"));
                        ui.strong(t!("logset.col.modified"));
                        ui.strong(t!("logset.col.file"));
                        ui.end_row();
                        for row in &mut dlg.rows {
                            let exists = row.state.as_ref().is_some_and(FileState::exists);
                            ui.add_enabled(exists, egui::Checkbox::without_text(&mut row.ticked));
                            match (&row.path, &row.state) {
                                (Err(var), _) => {
                                    no_wrap(
                                        ui,
                                        RichText::new(format!(
                                            "✖ {}",
                                            t!("logset.unknown_var.short")
                                        ))
                                        .color(RED),
                                    )
                                    .on_hover_text(t!("logset.unknown_var", name = var));
                                }
                                (Ok(_), None) => {
                                    ui.horizontal(|ui| {
                                        ui.add(egui::Spinner::new());
                                        ui.weak(t!("logset.checking"));
                                    });
                                }
                                (Ok(_), Some(s)) => {
                                    let (text, color, tip) = state_text(s, green);
                                    let r = no_wrap(ui, RichText::new(text).color(color));
                                    if let Some(tip) = tip {
                                        r.on_hover_text(tip);
                                    }
                                }
                            }
                            match &row.state {
                                Some(FileState::Found { size, modified, .. }) => {
                                    no_wrap(ui, RichText::new(format_size(*size)));
                                    no_wrap(
                                        ui,
                                        RichText::new(
                                            modified
                                                .as_deref()
                                                .map(|m| self.display_dt(m))
                                                .unwrap_or_default(),
                                        ),
                                    );
                                }
                                _ => {
                                    ui.label("");
                                    ui.label("");
                                }
                            }
                            let file = &row.file;
                            ui.horizontal(|ui| {
                                let shown = match &row.path {
                                    Ok(p) => p.display().to_string(),
                                    Err(_) => file.clone(),
                                };
                                no_wrap(ui, RichText::new(shown).monospace());
                                if let Some(FileState::Found { rotated, .. }) = &row.state
                                    && *rotated > 0
                                {
                                    no_wrap(
                                        ui,
                                        RichText::new(t!("logset.rotated", count = rotated)).weak(),
                                    );
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
    }
}

/// Text, color and tooltip (a long error message) of a checked file.
fn state_text(s: &FileState, green: Color32) -> (String, Color32, Option<String>) {
    match s {
        FileState::Found { .. } => (format!("✔ {}", t!("logset.found")), green, None),
        FileState::NotFound => (format!("✖ {}", t!("logset.not_found")), RED, None),
        FileState::AccessDenied => (format!("🔒 {}", t!("logset.access_denied")), RED, None),
        FileState::NotAFile => (format!("✖ {}", t!("logset.not_a_file")), RED, None),
        FileState::Error(e) => (
            format!("✖ {}", t!("logset.error.short")),
            RED,
            Some(t!("logset.error", error = e)),
        ),
    }
}
