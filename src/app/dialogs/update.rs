//! New versions (#15): check, download + install, restart with the same log and view.

use crate::app::*;

impl App {
    /// Asks GitHub for the latest release in a background thread.
    pub(in crate::app) fn start_update_check(&mut self, silent: bool) {
        if matches!(
            self.update,
            UpdateState::Checking { .. } | UpdateState::Installing { .. }
        ) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("rustrace-update".into())
            .spawn(move || {
                let _ = tx.send(update::fetch_latest());
                ctx.request_repaint();
            });
        self.update = match spawned {
            Ok(_) => UpdateState::Checking { rx, silent },
            Err(e) => UpdateState::Failed(e.to_string()),
        };
    }

    /// Takes over the results of the check / installation.
    pub(in crate::app) fn poll_update(&mut self, ctx: &egui::Context) {
        match &self.update {
            UpdateState::Checking { rx, silent } => {
                let silent = *silent;
                let res = match rx.try_recv() {
                    Ok(res) => res,
                    Err(mpsc::TryRecvError::Empty) => return,
                    Err(mpsc::TryRecvError::Disconnected) => Err(t!("error.read_internal")),
                };
                self.cfg.update_last_check = chrono::Local::now().timestamp();
                self.cfg_dirty = true;
                self.update = match res {
                    Ok(r) => {
                        let newer = update::is_newer(&r.version, &update::current_version());
                        self.update_available = newer.then(|| r.clone());
                        if silent && !(newer && r.version != self.cfg.update_skipped) {
                            UpdateState::Idle
                        } else {
                            self.show_update = true;
                            UpdateState::Checked(r)
                        }
                    }
                    // A failed automatic check (e.g. offline) is not reported.
                    Err(_) if silent => UpdateState::Idle,
                    Err(e) => UpdateState::Failed(e),
                };
            }
            UpdateState::Installing {
                rx, release, exe, ..
            } => {
                let res = match rx.try_recv() {
                    Ok(res) => res,
                    Err(mpsc::TryRecvError::Empty) => {
                        ctx.request_repaint_after(Duration::from_millis(100));
                        return;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => Err(t!("error.read_internal")),
                };
                let (release, exe) = (release.clone(), exe.clone());
                let res = res.and_then(|warnings| {
                    for w in warnings {
                        eprintln!("update: {w}");
                    }
                    // The settings first, the new program reads them.
                    let _ = self.cfg.save();
                    self.cfg_dirty = false;
                    update::restart(&exe, &self.session_state(ctx))
                        .map_err(|e| t!("update.restart_failed", error = e))
                });
                match res {
                    Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    Err(error) => self.update = UpdateState::InstallFailed { error, release },
                }
            }
            _ => {}
        }
    }

    /// Downloads and installs the release in a background thread.
    pub(in crate::app) fn start_install(&mut self, release: Release) {
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                self.update = UpdateState::InstallFailed {
                    error: e.to_string(),
                    release,
                };
                return;
            }
        };
        let progress = Arc::new(update::Progress::default());
        let (tx, rx) = mpsc::channel();
        let (prog, rel, path, ctx) = (
            progress.clone(),
            release.clone(),
            exe.clone(),
            self.ctx.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("rustrace-update".into())
            .spawn(move || {
                let _ = tx.send(update::download_and_install(&rel, &path, &prog));
                ctx.request_repaint();
            });
        self.update = match spawned {
            Ok(_) => UpdateState::Installing {
                rx,
                progress,
                release,
                exe,
            },
            Err(e) => UpdateState::InstallFailed {
                error: e.to_string(),
                release,
            },
        };
    }

    /// The open log and the view, for the restarted program.
    pub(in crate::app) fn session_state(&self, ctx: &egui::Context) -> SessionState {
        // egui points include the UI zoom, the window is placed in unzoomed logical pixels.
        let zoom = ctx.zoom_factor();
        let window = ctx.input(|i| {
            let vp = i.viewport();
            let maximized = vp.maximized.unwrap_or(false);
            WindowState {
                // A maximized window keeps its normal size from the default.
                pos: vp
                    .outer_rect
                    .filter(|_| !maximized)
                    .map(|r| [r.min.x * zoom, r.min.y * zoom]),
                size: vp
                    .inner_rect
                    .filter(|_| !maximized)
                    .map(|r| [r.width() * zoom, r.height() * zoom]),
                maximized,
            }
        });
        SessionState {
            path: self
                .cur_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            format: match &self.format_choice {
                FormatChoice::Auto => None,
                FormatChoice::Builtin => Some(FormatState::Builtin),
                FormatChoice::Def(d) => Some(FormatState::Def {
                    id: d.id.clone(),
                    priority: d.priority,
                }),
            },
            selected: self.selected,
            search_text: self.search_text.clone(),
            search_confirmed: self.search_confirmed,
            search_regex: self.search_regex,
            search_case: self.search_case,
            filter_mode: self.filter_mode,
            show_sev: self.show_sev,
            col_filters: self
                .col_filter_text
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            time_from: self.time_from.clone(),
            time_to: self.time_to.clone(),
            window: Some(window),
        }
    }

    /// Opens the log of the previous version with its search, filters and format; the selection
    /// and the column filters follow after the first read (`restore_view`).
    pub(in crate::app) fn restore_session(&mut self, state: SessionState) {
        self.search_text = state.search_text.clone();
        self.search_confirmed = state.search_confirmed;
        self.search_regex = state.search_regex;
        self.search_case = state.search_case;
        self.filter_mode = state.filter_mode;
        self.show_sev = state.show_sev;
        self.time_from = state.time_from.clone();
        self.time_to = state.time_to.clone();
        self.apply_time_range();
        self.recompile_search();
        let Some(path) = state.path.clone() else {
            return;
        };
        self.open_file(&path);
        let format = match &state.format {
            None => None,
            Some(FormatState::Builtin) => Some(FormatChoice::Builtin),
            Some(FormatState::Def { id, priority }) => self
                .defs
                .iter()
                .find(|d| d.id.eq_ignore_ascii_case(id) && d.priority == *priority)
                .map(|d| FormatChoice::Def(d.clone())),
        };
        if let Some(f) = format {
            self.set_format(f);
        }
        self.pending_restore = Some(state);
    }

    pub(in crate::app) fn restore_view(&mut self, state: SessionState) {
        if !state.col_filters.is_empty() {
            self.col_filter_text = state.col_filters.into_iter().collect();
            self.apply_column_filters();
        }
        if let Some(sel) = state.selected.filter(|&s| s < self.entries.len()) {
            self.selected = Some(sel);
            if !self.cfg.auto_scroll {
                self.scroll_to_selected_after_filter();
            }
        }
    }

    /// Help → Check for updates.
    pub(in crate::app) fn update_window(&mut self, ctx: &egui::Context) {
        if !self.show_update {
            return;
        }
        let installing = matches!(self.update, UpdateState::Installing { .. });
        let mut open = true;
        let mut action = None;
        let mut auto_changed = false;
        egui::Window::new(t!("update.title"))
            .id(egui::Id::unique("update_window"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(t!("update.current", version = update::current_version()));
                ui.add_space(6.0);
                match &self.update {
                    UpdateState::Idle => {}
                    UpdateState::Checking { .. } => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(t!("update.checking"));
                        });
                    }
                    UpdateState::Failed(e) => {
                        ui.colored_label(RED, t!("update.check_failed", error = e));
                    }
                    UpdateState::Checked(r)
                        if update::is_newer(&r.version, &update::current_version()) =>
                    {
                        ui.label(
                            RichText::new(t!("update.new_version", version = r.version)).strong(),
                        );
                        if !r.notes.trim().is_empty() {
                            ui.add_space(4.0);
                            egui::ScrollArea::vertical()
                                .max_height(260.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    for (heading, line) in update::plain_notes(&r.notes) {
                                        if heading {
                                            ui.add_space(4.0);
                                            ui.label(RichText::new(line).strong());
                                        } else {
                                            ui.add(egui::Label::new(line).wrap());
                                        }
                                    }
                                });
                        }
                        ui.add_space(6.0);
                        let can_install = cfg!(windows) && r.zip.is_some() && r.sha256.is_some();
                        if can_install {
                            let dir = std::env::current_exe()
                                .ok()
                                .and_then(|p| p.parent().map(|d| d.display().to_string()))
                                .unwrap_or_default();
                            ui.add(egui::Label::new(t!("update.install_hint", dir = dir)).wrap());
                        } else {
                            ui.colored_label(ORANGE, t!("update.no_package"));
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    can_install,
                                    egui::Button::new(format!("⬇ {}", t!("update.install"))),
                                )
                                .clicked()
                            {
                                action = Some(UpdateAction::Install(r.clone()));
                            }
                            if ui
                                .button(format!("🌐 {}", t!("update.release_page")))
                                .clicked()
                            {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(&r.page));
                            }
                            if ui.button(t!("update.skip")).clicked() {
                                action = Some(UpdateAction::Skip(r.version.clone()));
                            }
                            if ui.button(t!("update.later")).clicked() {
                                action = Some(UpdateAction::Close);
                            }
                        });
                    }
                    UpdateState::Checked(r) => {
                        ui.colored_label(
                            Color32::from_rgb(60, 170, 80),
                            format!("✔ {}", t!("update.up_to_date", version = r.version)),
                        );
                        ui.hyperlink_to(t!("update.release_page"), &r.page);
                    }
                    UpdateState::Installing {
                        progress, release, ..
                    } => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(t!("update.downloading", version = release.version));
                        });
                        let received = progress.received.load(Ordering::Relaxed);
                        let total = progress.total.load(Ordering::Relaxed);
                        let (fraction, text) = if total > 0 {
                            let text =
                                format!("{} / {}", format_size(received), format_size(total));
                            (received as f32 / total as f32, text)
                        } else {
                            (0.0, format_size(received))
                        };
                        ui.add(egui::ProgressBar::new(fraction).text(text));
                        if ui.button(t!("update.cancel")).clicked() {
                            progress.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    UpdateState::InstallFailed { error, release } => {
                        ui.colored_label(RED, t!("update.install_failed", error = error));
                        ui.add(egui::Label::new(t!("update.install_failed_hint")).wrap());
                        ui.hyperlink_to(t!("update.release_page"), &release.page);
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let busy = matches!(
                        self.update,
                        UpdateState::Checking { .. } | UpdateState::Installing { .. }
                    );
                    if ui
                        .add_enabled(
                            !busy,
                            egui::Button::new(format!("🔄 {}", t!("update.check_again"))),
                        )
                        .clicked()
                    {
                        action = Some(UpdateAction::Check);
                    }
                });
                ui.separator();
                auto_changed = ui
                    .checkbox(&mut self.cfg.update_check, t!("update.auto"))
                    .changed();
            });
        if auto_changed {
            self.cfg_dirty = true;
        }
        // The window stays open while the update is being installed.
        self.show_update = open || installing;
        match action {
            Some(UpdateAction::Install(r)) => self.start_install(r),
            Some(UpdateAction::Check) => self.start_update_check(false),
            Some(UpdateAction::Skip(v)) => {
                self.cfg.update_skipped = v;
                self.cfg_dirty = true;
                self.update_available = None;
                self.show_update = false;
            }
            Some(UpdateAction::Close) => self.show_update = false,
            None => {}
        }
    }
}

/// A button pressed in the update window.
pub(in crate::app) enum UpdateAction {
    Install(Release),
    Check,
    Skip(String),
    Close,
}
