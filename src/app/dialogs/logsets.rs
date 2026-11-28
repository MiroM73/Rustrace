//! File → Open log set (#27): a predefined group of logs for a troubleshooting scenario, on this
//! computer or on a remote one (over its administrative shares), opened as one view merged by
//! time. Which files exist is checked in a background thread; the shares of a remote computer
//! must answer within `logset::REMOTE_TIMEOUT`, so an offline computer never blocks the window.
//! The sets are listed on the left with a search field; the name and description of an own set
//! can be edited (#77).

use crate::app::*;
use crate::logset::{self, FileState, LogSetDef};
use crate::logsetsave::{self, Changes};

/// State of the open "Open log set" window.
pub(in crate::app) struct LogSetDialog {
    sets: Vec<LogSetDef>,
    errors: Vec<String>,
    chosen: usize,
    /// The search text: the list shows only the sets it finds.
    query: String,
    focus_search: bool,
    /// The name and description of the chosen own set are being edited.
    edit: Option<SetEdit>,
    /// A remote computer is chosen and its name as typed.
    remote: bool,
    computer: String,
    /// The computer the rows belong to (`None` = this computer).
    checked_pc: Option<String>,
    rows: Vec<SetRow>,
    /// The running check, when it started and whether the shares have answered.
    check: Option<mpsc::Receiver<CheckMsg>>,
    started: Instant,
    shares_pending: bool,
    /// The first share that could not be used: (share, why).
    share_problem: Option<(String, FileState)>,
}

/// Editing the name and description of an own set.
struct SetEdit {
    path: PathBuf,
    name: String,
    description: String,
    /// As they were shown when editing began: only what differs is written.
    old_name: String,
    old_description: String,
    error: Option<String>,
}

impl SetEdit {
    /// Another own set has the new name.
    fn name_taken(&self, sets: &[LogSetDef]) -> bool {
        let name = self.name.trim();
        name != self.old_name
            && sets
                .iter()
                .any(|s| s.is_own() && s.path.as_ref() != Some(&self.path) && s.has_name(name))
    }
}

/// Results of the background check.
enum CheckMsg {
    /// Every share of the paths and whether it answers (sent first).
    Shares(Vec<(String, Result<(), FileState>)>),
    /// One state per row with a resolved path, in row order.
    Files(Vec<FileState>),
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

    /// Takes over the results of the background check; the shares that do not answer in time
    /// mark the rows as timed out.
    fn poll_check(&mut self, ctx: &egui::Context) {
        while let Some(msg) = self.check.as_ref().and_then(|rx| rx.try_recv().ok()) {
            match msg {
                CheckMsg::Shares(shares) => {
                    self.shares_pending = false;
                    self.share_problem = shares
                        .into_iter()
                        .find_map(|(share, r)| r.err().map(|e| (share, e)));
                }
                CheckMsg::Files(states) => {
                    let mut states = states.into_iter();
                    for row in self.rows.iter_mut().filter(|r| r.path.is_ok()) {
                        row.state = states.next();
                        row.ticked = row.state.as_ref().is_some_and(FileState::exists);
                    }
                    self.check = None;
                }
            }
        }
        if self.check.is_some() && self.shares_pending {
            let left = logset::REMOTE_TIMEOUT.saturating_sub(self.started.elapsed());
            if left.is_zero() {
                // The thread may still hang in the network call; its result is dropped.
                self.check = None;
                self.shares_pending = false;
                for row in self.rows.iter_mut().filter(|r| r.path.is_ok()) {
                    row.state = Some(FileState::TimedOut);
                }
                let pc = self.checked_pc.clone().unwrap_or_default();
                self.share_problem = Some((format!(r"\\{pc}"), FileState::TimedOut));
            } else {
                ctx.request_repaint_after(left);
            }
        }
    }
}

/// The text above the buttons when a share of the computer cannot be used.
fn share_problem_text(share: &str, state: &FileState) -> String {
    let pc = logset::share_host(share);
    match state {
        FileState::TimedOut => t!(
            "logset.remote.timeout",
            pc = pc,
            seconds = logset::REMOTE_TIMEOUT.as_secs()
        ),
        FileState::AccessDenied => t!("logset.remote.access_denied", share = share),
        FileState::NoShare => t!("logset.remote.no_share", share = share),
        FileState::Error(e) => t!("logset.remote.error", share = share, error = e),
        _ => t!("logset.remote.unreachable", pc = pc),
    }
}

impl App {
    /// File → Open log set: (re)loads the set files and checks the last chosen set.
    pub(in crate::app) fn open_log_sets(&mut self) {
        let (sets, errors) = self.sorted_log_sets();
        let chosen = self.last_set_index(&sets);
        let mut dlg = LogSetDialog {
            sets,
            errors,
            chosen,
            query: String::new(),
            focus_search: true,
            edit: None,
            remote: self.cfg.log_set_remote,
            computer: self
                .cfg
                .remote_computers
                .first()
                .cloned()
                .unwrap_or_default(),
            checked_pc: None,
            rows: Vec::new(),
            check: None,
            started: Instant::now(),
            shares_pending: false,
            share_problem: None,
        };
        self.start_set_check(&mut dlg);
        self.log_sets = Some(dlg);
        self.log_sets_win.open();
    }

    /// Reads the set files again in the open window (a set was saved), keeping the computer and
    /// choosing the set saved last.
    pub(in crate::app) fn reload_log_sets(&mut self) {
        let Some(mut dlg) = self.log_sets.take() else {
            return;
        };
        let (sets, errors) = self.sorted_log_sets();
        dlg.chosen = self.last_set_index(&sets);
        dlg.sets = sets;
        dlg.errors = errors;
        dlg.edit = None;
        self.start_set_check(&mut dlg);
        self.log_sets = Some(dlg);
    }

    /// All sets – predefined first, then own, each group by its name in the current language –
    /// and the errors of the files that cannot be used.
    fn sorted_log_sets(&self) -> (Vec<LogSetDef>, Vec<String>) {
        let list = logset::load_all();
        let mut sets = list.sets;
        sets.sort_by_cached_key(|s| (s.is_own(), s.name(&self.lang_code).to_lowercase()));
        (sets, list.errors)
    }

    fn last_set_index(&self, sets: &[LogSetDef]) -> usize {
        sets.iter()
            .position(|s| s.key() == self.cfg.last_log_set)
            .unwrap_or(0)
    }

    /// Resolves the files of the chosen set for the chosen computer and checks them in a
    /// background thread: first the shares of the paths, then every file.
    fn start_set_check(&self, dlg: &mut LogSetDialog) {
        dlg.check = None;
        dlg.share_problem = None;
        dlg.shares_pending = false;
        let vars = if dlg.remote {
            match logset::computer_name(&dlg.computer) {
                Some(pc) => logset::remote_vars(&pc),
                None => {
                    dlg.rows.clear();
                    dlg.checked_pc = None;
                    return;
                }
            }
        } else {
            logset::this_computer_vars()
        };
        dlg.checked_pc = vars.remote.clone();
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
        let mut shares: Vec<String> = Vec::new();
        for p in &paths {
            if let Some(s) = logset::share_root(&p.to_string_lossy())
                && !shares.iter().any(|x| x.eq_ignore_ascii_case(&s))
            {
                shares.push(s);
            }
        }
        dlg.shares_pending = !shares.is_empty();
        dlg.started = Instant::now();
        let rotated = self.cfg.merge_rotated;
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("rustrace-logset".into())
            .spawn(move || {
                let answers: Vec<(String, Result<(), FileState>)> = shares
                    .into_iter()
                    .map(|s| {
                        let r = logset::check_share(&s);
                        (s, r)
                    })
                    .collect();
                if tx.send(CheckMsg::Shares(answers.clone())).is_err() {
                    return;
                }
                ctx.request_repaint();
                let states = paths
                    .iter()
                    .map(|p| {
                        let share = logset::share_root(&p.to_string_lossy());
                        let failed = answers.iter().find_map(|(s, r)| {
                            share
                                .as_ref()
                                .filter(|x| x.eq_ignore_ascii_case(s))
                                .and_then(|_| r.clone().err())
                        });
                        failed.unwrap_or_else(|| logset::check_file(p, rotated))
                    })
                    .collect();
                let _ = tx.send(CheckMsg::Files(states));
                ctx.request_repaint();
            });
        dlg.check = spawned.is_ok().then_some(rx);
    }

    pub(in crate::app) fn log_sets_window(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.log_sets.take() else {
            self.log_sets_win.close();
            return;
        };
        dlg.poll_check(ctx);
        let (mut chosen, mut recheck, mut open_logs, mut close, mut own_folder) =
            (None, false, false, false, false);
        let mut save_edit = false;
        let mut guide: Option<String> = None;
        let lang = self.lang_code.clone();
        let recent = self.cfg.remote_computers.clone();
        // The panes have their own scroll areas.
        let spec = ToolSpec::new(
            format!("📑 {}", t!("logset.title")),
            [1100.0, 560.0],
            [720.0, 360.0],
        )
        .no_scroll();
        let open = self.show_tool(
            ctx,
            |a| &mut a.log_sets_win,
            spec,
            |app, ui, _keep| {
                egui::Panel::bottom("logset_buttons").show(ui, |ui| {
                    ui.add_space(4.0);
                    let count = dlg
                        .rows
                        .iter()
                        .filter(|r| r.ticked && r.state.as_ref().is_some_and(FileState::exists))
                        .count();
                    if let Some((share, state)) = &dlg.share_problem {
                        ui.colored_label(RED, share_problem_text(share, state));
                    } else if dlg.check.is_none()
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
                                egui::Button::new(format!(
                                    "📂 {}",
                                    t!("logset.open", count = count)
                                )),
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
                        egui::ScrollArea::vertical()
                            .id_salt("logset_errors")
                            .max_height(60.0)
                            .show(ui, |ui| {
                                for e in &dlg.errors {
                                    ui.label(RichText::new(e).small().color(RED));
                                }
                            });
                    }
                });
                egui::Panel::left("logset_list")
                    .resizable(true)
                    .default_size(280.0)
                    .min_size(180.0)
                    .show(ui, |ui| chosen = set_list(ui, &mut dlg, &lang));
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if let Some(s) = dlg.set() {
                            ui.heading(s.name(&lang));
                            if s.is_own()
                                && dlg.edit.is_none()
                                && ui
                                    .button(format!("✏ {}", t!("logset.edit")))
                                    .on_hover_text(t!("logset.edit.tip"))
                                    .clicked()
                                && let Some(path) = s.path.clone()
                            {
                                let name = s.name(&lang);
                                let description =
                                    s.description(&lang).unwrap_or_default().to_owned();
                                dlg.edit = Some(SetEdit {
                                    path,
                                    old_name: name.clone(),
                                    old_description: description.clone(),
                                    name,
                                    description,
                                    error: None,
                                });
                            }
                        }
                        // Guides of the scenario (#39): built-in ones by their title, others by
                        // name.
                        for g in dlg.set().map(|s| s.guides.as_slice()).unwrap_or_default() {
                            let title = guides::title_key(g).map_or_else(|| g.clone(), |k| t!(k));
                            let target = guides::target(REPOSITORY, g);
                            let tip = if guides::title_key(g).is_some() {
                                t!("menu.help.guides.tip")
                            } else {
                                target.clone()
                            };
                            if ui
                                .button(format!("📖 {title}"))
                                .on_hover_text(tip)
                                .clicked()
                            {
                                guide = Some(target);
                            }
                        }
                    });
                    if dlg.edit.is_some() {
                        save_edit = edit_form(ui, &mut dlg);
                    } else if let Some(d) = dlg.set().and_then(|s| s.description(&lang)) {
                        ui.add(egui::Label::new(RichText::new(d).weak()).wrap());
                    }
                    if let Some(p) = dlg.set().and_then(|s| s.path.as_ref()) {
                        ui.weak(t!("logset.source", path = p.display()));
                    }
                    recheck |= computer_row(ui, &mut dlg, &recent);
                    ui.separator();
                    if dlg.remote && dlg.checked_pc.is_none() {
                        let text = if dlg.computer.trim().is_empty() {
                            t!("logset.remote.enter")
                        } else {
                            t!("logset.remote.invalid", name = dlg.computer.trim())
                        };
                        ui.colored_label(ORANGE, text);
                    } else {
                        app.set_rows(ui, &mut dlg);
                    }
                });
            },
        );

        if save_edit {
            self.save_set_edit(&mut dlg);
        }
        if let Some(link) = guide {
            self.open_link(&link);
        }
        if own_folder && let Some(dir) = logset::user_set_dir() {
            let res = std::fs::create_dir_all(&dir).and_then(|_| open::that_detached(&dir));
            if let Err(e) = res {
                self.status_error = Some(t!("error.open_folder", error = e));
            }
        }
        if let Some(i) = chosen.filter(|&i| i != dlg.chosen) {
            dlg.chosen = i;
            dlg.edit = None;
            if let Some(s) = dlg.set() {
                self.cfg.last_log_set = s.key();
                self.cfg_dirty = true;
            }
            recheck = true;
        }
        if self.cfg.log_set_remote != dlg.remote {
            self.cfg.log_set_remote = dlg.remote;
            self.cfg_dirty = true;
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
            }
            if let Some(pc) = &dlg.checked_pc {
                self.cfg.push_computer(pc);
            }
            self.cfg_dirty = true;
            let set_name = dlg.set().map(|s| s.name(&self.lang_code));
            self.open_files(paths);
            self.set_name = set_name;
            self.log_sets_win.close();
            return;
        }
        if open && !close {
            self.log_sets = Some(dlg);
        } else {
            self.log_sets_win.close();
        }
    }

    /// ✔ of the edit form: writes the name and description into the file of the set, keeping
    /// the rest of it, and reads the sets again.
    fn save_set_edit(&mut self, dlg: &mut LogSetDialog) {
        let Some(edit) = &dlg.edit else {
            return;
        };
        let (name, description) = (edit.name.trim(), edit.description.trim());
        let changes = Changes {
            name: (name != edit.old_name).then_some(name),
            description: (description != edit.old_description).then_some(description),
            files: None,
        };
        if changes.name.is_none() && changes.description.is_none() {
            dlg.edit = None;
            return;
        }
        if let Err(e) = logsetsave::save(&edit.path, true, &self.lang_code, &changes) {
            let text = t!("logset.save.error", file = edit.path.display(), error = e);
            if let Some(edit) = &mut dlg.edit {
                edit.error = Some(text);
            }
            return;
        }
        // The window title follows a renamed set whose logs are open.
        if changes.name.is_some() && self.set_name.as_ref() == Some(&edit.old_name) {
            self.set_name = Some(name.to_owned());
        }
        let key = dlg.set().map(LogSetDef::key);
        let (sets, errors) = self.sorted_log_sets();
        // The order may have changed; the logs of the set have not.
        dlg.chosen = sets.iter().position(|s| Some(s.key()) == key).unwrap_or(0);
        dlg.sets = sets;
        dlg.errors = errors;
        dlg.edit = None;
    }

    /// The table of the logs of the set: tick, state, path, size, last write time.
    fn set_rows(&self, ui: &mut egui::Ui, dlg: &mut LogSetDialog) {
        let green = if self.dark {
            Color32::from_rgb(110, 200, 110)
        } else {
            Color32::from_rgb(20, 130, 40)
        };
        let no_wrap = |ui: &mut egui::Ui, text: RichText| ui.add(egui::Label::new(text).extend());
        egui::ScrollArea::both()
            .id_salt("logset_rows")
            .auto_shrink([false, false])
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

/// The search field and the list of the sets it finds; returns the set that was clicked.
fn set_list(ui: &mut egui::Ui, dlg: &mut LogSetDialog, lang: &str) -> Option<usize> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.label("🔍");
        // The button first, so the field takes exactly the rest (a wider one would make the
        // panel grow).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !dlg.query.is_empty() && ui.small_button("✖").clicked() {
                dlg.query.clear();
            }
            let r = ui.add(
                egui::TextEdit::singleline(&mut dlg.query)
                    // The button appearing before it must not change its id (focus).
                    .id_salt("logset_search")
                    .hint_text(t!("logset.search.hint"))
                    .desired_width(f32::INFINITY),
            );
            if dlg.focus_search {
                r.request_focus();
                dlg.focus_search = false;
            }
        });
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("logset_sets")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Full width entries with the names on the left.
            let layout = egui::Layout::top_down_justified(egui::Align::LEFT);
            ui.with_layout(layout, |ui| clicked = set_entries(ui, dlg, lang));
        });
    clicked
}

/// The entries of the list of sets: name and, under it, the description.
fn set_entries(ui: &mut egui::Ui, dlg: &LogSetDialog, lang: &str) -> Option<usize> {
    let mut clicked = None;
    let mut any = false;
    for (i, s) in dlg.sets.iter().enumerate() {
        if !s.found_by(&dlg.query) {
            continue;
        }
        any = true;
        let name = if s.is_own() {
            format!("{} ({})", s.name(lang), t!("logset.own"))
        } else {
            s.name(lang)
        };
        let button = egui::Button::selectable(i == dlg.chosen, name).truncate();
        let mut r = ui.add(button);
        if let Some(d) = s.description(lang) {
            let text = egui::Label::new(RichText::new(d).small().weak())
                .truncate()
                .selectable(false)
                .sense(egui::Sense::click());
            r |= ui.add(text).on_hover_text(d);
        }
        if r.clicked() {
            clicked = Some(i);
        }
        ui.add_space(2.0);
    }
    if !any {
        ui.weak(t!("logset.search.none"));
    }
    clicked
}

/// Name and description of the edited set with ✔ / ✖; true = save.
fn edit_form(ui: &mut egui::Ui, dlg: &mut LogSetDialog) -> bool {
    let Some(edit) = &mut dlg.edit else {
        return false;
    };
    let taken = edit.name_taken(&dlg.sets);
    let (mut save, mut cancel) = (false, false);
    egui::Grid::new("logset_edit")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label(t!("logset.save.name"));
            ui.add(egui::TextEdit::singleline(&mut edit.name).desired_width(420.0));
            ui.end_row();
            ui.label(t!("logset.save.description"));
            ui.add(
                egui::TextEdit::multiline(&mut edit.description)
                    .hint_text(t!("logset.save.description.hint"))
                    .desired_rows(2)
                    .desired_width(420.0),
            );
            ui.end_row();
        });
    if taken {
        ui.colored_label(RED, t!("logset.name_taken"));
    }
    if let Some(e) = &edit.error {
        ui.colored_label(RED, e);
    }
    ui.horizontal(|ui| {
        let ok = !taken && !edit.name.trim().is_empty();
        save = ui
            .add_enabled(
                ok,
                egui::Button::new(format!("✔ {}", t!("logset.save.button"))),
            )
            .clicked();
        cancel = ui.button(format!("✖ {}", t!("progress.cancel"))).clicked();
    });
    if cancel {
        dlg.edit = None;
    }
    save
}

/// Text, color and tooltip (a long error message) of a checked file.
fn state_text(s: &FileState, green: Color32) -> (String, Color32, Option<String>) {
    match s {
        FileState::Found { .. } => (format!("✔ {}", t!("logset.found")), green, None),
        FileState::NotFound => (format!("✖ {}", t!("logset.not_found")), RED, None),
        FileState::AccessDenied => (format!("🔒 {}", t!("logset.access_denied")), RED, None),
        FileState::NotAFile => (format!("✖ {}", t!("logset.not_a_file")), RED, None),
        FileState::Unreachable => (format!("✖ {}", t!("logset.unreachable")), RED, None),
        FileState::NoShare => (format!("✖ {}", t!("logset.no_share")), RED, None),
        FileState::TimedOut => (format!("⏱ {}", t!("logset.timed_out")), RED, None),
        FileState::Error(e) => (
            format!("✖ {}", t!("logset.error.short")),
            RED,
            Some(t!("logset.error", error = e)),
        ),
    }
}

/// "Computer: (•) This computer ( ) Remote computer [name] [🕘] [Check]"; true = check again.
fn computer_row(ui: &mut egui::Ui, dlg: &mut LogSetDialog, recent: &[String]) -> bool {
    let mut recheck = false;
    ui.horizontal(|ui| {
        ui.label(t!("logset.computer"));
        if ui.radio(!dlg.remote, t!("logset.local")).clicked() && dlg.remote {
            dlg.remote = false;
            recheck = true;
        }
        if ui
            .radio(dlg.remote, t!("logset.remote"))
            .on_hover_text(t!("logset.remote.tip"))
            .clicked()
            && !dlg.remote
        {
            dlg.remote = true;
            recheck = true;
        }
        if !dlg.remote {
            return;
        }
        let r = ui.add(
            egui::TextEdit::singleline(&mut dlg.computer)
                .hint_text(t!("logset.remote.hint"))
                .desired_width(220.0),
        );
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            recheck = true;
        }
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button("🕘", |ui| {
                for c in recent {
                    if ui.button(c).clicked() {
                        dlg.computer = c.clone();
                        recheck = true;
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text(t!("logset.remote.recent"));
        });
        if ui
            .add_enabled(
                dlg.check.is_none(),
                egui::Button::new(format!("🔍 {}", t!("logset.remote.check"))),
            )
            .clicked()
        {
            recheck = true;
        }
    });
    recheck
}
