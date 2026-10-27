//! Opening files, background reading, auto refresh, rotation and memory limits.

use crate::app::*;

impl App {
    /// Protection of a long-watched log against filling the memory:
    /// 1. message text that grew past the temp file threshold moves to the temporary file,
    /// 2. above the size limit the oldest entries are removed (down to ¾ of the limit, so this
    ///    happens rarely and in one go).
    pub(in crate::app) fn enforce_memory_limits(&mut self) {
        let mb = |v: u32| (v as usize) << 20;
        if self.cfg.temp_file_enabled
            && !self.entries.is_mapped()
            && self.entries.text_len() >= mb(self.cfg.temp_file_threshold_mb)
        {
            self.entries.move_text_to_temp_file();
        }
        if !self.cfg.max_log_enabled {
            return;
        }
        let limit = mb(self.cfg.max_log_mb.max(1));
        let len = self.entries.text_len();
        if len <= limit {
            return;
        }
        let k = self
            .entries
            .first_at_or_after(len - limit / 4 * 3)
            .min(self.committed_len);
        if k == 0 {
            return;
        }
        let pos_selected = self
            .selected
            .and_then(|s| self.filtered.binary_search(&s).ok());
        self.entries.drop_front(k);
        self.committed_len -= k;
        self.dropped += k;
        self.data_gen += 1;
        // Shift every entry index by k; indexes of removed entries disappear.
        let gone = self.filtered.partition_point(|&i| i < k);
        self.filtered.drain(..gone);
        for i in &mut self.filtered {
            *i -= k;
        }
        self.is_match.drain(..k.min(self.is_match.len()));
        self.match_count = self.is_match.iter().filter(|&&b| b).count();
        self.selected = self.selected.and_then(|s| s.checked_sub(k));
        self.code_cache = None;
        // Keep the selected row in view (the rows above it disappeared).
        if !self.cfg.auto_scroll
            && let Some(p) = pos_selected.and_then(|p| p.checked_sub(gone))
        {
            self.scroll_to = Some(p);
        }
    }

    /// Table columns: the columns of the log type, plus the File column first when the log is
    /// merged from several files (rotated parts).
    pub(in crate::app) fn columns(&self) -> Vec<LogColumn> {
        let mut cols = self.schema.columns.clone();
        if self.files.len() > 1 {
            cols.insert(
                0,
                LogColumn {
                    kind: ColumnKind::File,
                    title: Some(t!("column.file")),
                },
            );
        }
        cols
    }

    pub(in crate::app) fn set_files(&mut self, files: Vec<SourceFile>) {
        self.files = files;
        self.file_labels = (0..self.files.len() as u16)
            .map(|i| self.file_label(i))
            .collect();
    }

    /// Name of a file of the log ("(overwritten)" when a later rotation replaced it).
    pub(in crate::app) fn file_label(&self, id: u16) -> String {
        match self.files.get(id as usize) {
            Some(f) if f.path.is_some() => f.name.clone(),
            Some(f) => t!("file.overwritten", name = f.name),
            None => String::new(),
        }
    }

    pub(in crate::app) fn open_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter(t!("dialog.filter.logs"), &["log", "lo_", "txt"])
            .add_filter(t!("dialog.filter.all"), &["*"]);
        if let Some(dir) = self
            .cur_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        {
            dlg = dlg.set_directory(dir);
        } else if cfg!(windows) && std::path::Path::new(r"C:\Windows\CCM\Logs").exists() {
            dlg = dlg.set_directory(r"C:\Windows\CCM\Logs");
        }
        if let Some(p) = dlg.pick_file() {
            self.open_file(&p.to_string_lossy());
        }
    }

    pub(in crate::app) fn open_file(&mut self, path: &str) {
        self.cancel_loading();
        self.data_gen += 1;
        self.cur_path = Some(PathBuf::from(path));
        self.source = Some(LogSource::new(path, self.cfg.keyword_severity));
        self.format_choice = FormatChoice::Auto;
        self.active_def = None;
        self.schema = Schema::default();
        self.set_files(Vec::new());
        self.dropped = 0;
        self.entries = LogStore::new();
        self.filtered.clear();
        self.is_match.clear();
        self.match_count = 0;
        self.committed_len = 0;
        self.selected = None;
        self.code_cache = None;
        self.last_refresh = None;
        self.last_refresh_wall = None;
        self.file_size = 0;
        self.status_error = None;
        self.pending_recent = Some(path.to_owned());
        self.start_poll();
    }

    /// Starts reading the file in a background thread (unless one is already running).
    pub(in crate::app) fn start_poll(&mut self) {
        if self.loading.is_some() {
            return;
        }
        let Some(mut src) = self.source.take() else {
            return;
        };
        src.keyword_severity = self.cfg.keyword_severity;
        src.defs = self.defs.clone();
        src.format = self.format_choice.clone();
        src.merge_rotated = self.cfg.merge_rotated;
        src.temp_file_threshold = self
            .cfg
            .temp_file_enabled
            .then(|| u64::from(self.cfg.temp_file_threshold_mb) << 20);
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let prog = progress.clone();
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("rustrace-reader".into())
            .spawn(move || {
                let res = src.poll(&prog);
                // Nothing new: no extra frame, the next scheduled refresh picks the result up.
                let changed = !matches!(res, Ok(PollResult::Unchanged));
                // If the UI no longer waits for the result (other file, cancelled), send fails – that's fine.
                let _ = tx.send((src, res));
                if changed {
                    ctx.request_repaint();
                }
            });
        match spawned {
            Ok(_) => {
                self.loading = Some(Loading {
                    rx,
                    progress,
                    started: Instant::now(),
                });
            }
            Err(e) => {
                self.status_error = Some(t!("error.thread", error = e));
                self.recreate_source();
            }
        }
    }

    /// Takes over the result from the background thread when it is ready.
    pub(in crate::app) fn check_loading(&mut self) {
        let Some(l) = &self.loading else { return };
        let started = l.started;
        match l.rx.try_recv() {
            Ok((src, res)) => {
                self.loading = None;
                self.encoding_name = src.encoding_name();
                self.file_size = src.file_size;
                if self.schema != *src.schema() {
                    // Other columns (another log type): the column filters no longer apply.
                    if self.schema.columns != src.schema().columns {
                        self.col_filter_text.clear();
                        self.col_filters.clear();
                    }
                    self.schema = src.schema().clone();
                }
                self.active_def = src.active_def();
                if self.files != src.files() {
                    self.set_files(src.files().to_vec());
                }
                self.source = Some(src);
                self.apply_result(res);
                self.enforce_memory_limits();
                if let Some(state) = self.pending_restore.take() {
                    self.restore_view(state);
                }
                // The interval counts from the start of the read: an unchanged result is only picked
                // up by the next scheduled frame, which must already start the next read.
                self.last_refresh = Some(started);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                // The thread crashed – start over with the next refresh.
                self.loading = None;
                self.status_error = Some(t!("error.read_internal"));
                self.recreate_source();
                self.last_refresh = Some(Instant::now());
            }
        }
    }

    pub(in crate::app) fn apply_result(&mut self, res: std::io::Result<PollResult>) {
        let before = self.filtered.len();
        // Provisional entries belong to the active file (the last one).
        let active = self.files.len().saturating_sub(1) as u16;
        if matches!(
            res,
            Ok(PollResult::Reset { .. }) | Ok(PollResult::Appended { .. })
        ) {
            self.data_gen += 1;
        }
        match res {
            Ok(PollResult::Unchanged) => {
                self.status_error = None;
            }
            Ok(PollResult::Reset {
                committed,
                provisional,
            }) => {
                self.status_error = None;
                self.entries = committed;
                self.dropped = 0;
                self.committed_len = self.entries.len();
                for e in provisional {
                    self.entries.push(e, active);
                }
                self.selected = None;
                self.code_cache = None;
                self.filtered.clear();
                self.refilter_from(0);
                self.last_refresh_wall = Some(chrono::Local::now());
                if let Some(p) = self.pending_recent.take() {
                    self.cfg.push_recent(&p);
                    self.cfg_dirty = true;
                }
                if self.cfg.auto_scroll && !self.filtered.is_empty() {
                    self.scroll_to = Some(self.filtered.len() - 1);
                }
            }
            Ok(PollResult::Appended {
                committed,
                provisional,
            }) => {
                self.status_error = None;
                let start = self.committed_len;
                self.entries.truncate(start);
                self.entries.append(committed);
                self.committed_len = self.entries.len();
                for e in provisional {
                    self.entries.push(e, active);
                }
                if self.selected.is_some_and(|s| s >= start) {
                    self.code_cache = None;
                }
                self.refilter_from(start);
                self.last_refresh_wall = Some(chrono::Local::now());
                if self.cfg.auto_scroll
                    && self.filtered.len() != before
                    && !self.filtered.is_empty()
                {
                    self.scroll_to = Some(self.filtered.len() - 1);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => {
                self.status_error = Some(t!("error.read", error = e));
            }
        }
    }

    /// New source for the current file (the next read starts from the beginning).
    pub(in crate::app) fn recreate_source(&mut self) {
        self.source = self
            .cur_path
            .as_ref()
            .map(|p| LogSource::new(p, self.cfg.keyword_severity));
    }

    /// Cancels loading in progress (the thread stops at the next chunk).
    pub(in crate::app) fn cancel_loading(&mut self) {
        if let Some(l) = self.loading.take() {
            l.progress.cancel.store(true, Ordering::Relaxed);
            // The source stayed in the thread – create a new one, reading starts from the beginning.
            self.recreate_source();
        }
    }

    pub(in crate::app) fn refresh(&mut self) {
        self.start_poll();
    }

    pub(in crate::app) fn reload_full(&mut self) {
        self.cancel_loading();
        self.recreate_source();
        self.code_cache = None;
        self.start_poll();
    }

    pub(in crate::app) fn tick_auto_refresh(&mut self, ctx: &egui::Context) {
        if self.cur_path.is_none() {
            return;
        }
        if self.loading.is_some() {
            // Repaint while loading to show progress.
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        if !self.cfg.auto_refresh {
            return;
        }
        let interval = Duration::from_secs_f64(self.cfg.interval_ms() / 1000.0);
        let due = self.last_refresh.is_none_or(|t| t.elapsed() >= interval);
        if due {
            self.start_poll();
            ctx.request_repaint_after(interval);
            return;
        }
        let elapsed = self.last_refresh.map(|t| t.elapsed()).unwrap_or_default();
        ctx.request_repaint_after(
            interval
                .saturating_sub(elapsed)
                .max(Duration::from_millis(10)),
        );
    }

    /// Loading progress: big (middle of the window) or compact (status bar).
    pub(in crate::app) fn progress_ui(&mut self, ui: &mut egui::Ui, big: bool) {
        let Some(l) = &self.loading else { return };
        let total = l.progress.total.load(Ordering::Relaxed);
        let done = l.progress.done.load(Ordering::Relaxed).min(total);
        // Short refreshes (a few kB) are not shown, so the status bar does not flicker.
        if !big && (l.started.elapsed() < Duration::from_millis(300) || total < 4 * 1024 * 1024) {
            return;
        }
        let frac = if total > 0 {
            done as f32 / total as f32
        } else {
            0.0
        };
        let text = t!(
            "progress.text",
            percent = format!("{:.0}", frac * 100.0),
            done = format_size(done),
            total = format_size(total)
        );
        let cancel = if big {
            ui.heading(t!("progress.title"));
            ui.add_space(8.0);
            ui.add(egui::ProgressBar::new(frac).text(text).animate(true));
            ui.add_space(8.0);
            ui.button(t!("progress.cancel")).clicked()
        } else {
            // The separator only with the progress: a widget that appears for a moment on every
            // refresh would cost an extra frame each time.
            ui.separator();
            ui.spinner();
            ui.add(egui::ProgressBar::new(frac).text(text).desired_width(260.0));
            ui.small_button(t!("progress.cancel")).clicked()
        };
        if cancel {
            self.cancel_loading();
            self.cfg.auto_refresh = false;
            self.cfg_dirty = true;
            self.info(t!("progress.cancelled"));
        }
    }
}
