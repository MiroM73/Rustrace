//! Export of highlighted rows with context (Ctrl+Shift+S).

use crate::app::*;

impl App {
    /// Is the entry "highlighted" according to the export criteria?
    pub(in crate::app) fn is_export_hit(&self, idx: usize) -> bool {
        let e = self.entries.at(idx);
        let c = &self.cfg;
        (c.export_errors && e.severity == Severity::Error)
            || (c.export_warnings && e.severity == Severity::Warning)
            || (c.export_search && self.is_match.get(idx).copied().unwrap_or(false))
            || (c.export_rules
                && self.rules.iter().any(|r| {
                    r.regex.is_match(e.message)
                        || (r.rule.scope == HighlightScope::Row
                            && r.rule.match_component
                            && Self::other_columns_match(&r.regex, &e))
                }))
    }

    /// Indexes of highlighted entries (recomputed only when something they depend on changed).
    pub(in crate::app) fn export_hits(&mut self) -> &[usize] {
        let key = ExportKey {
            data_gen: self.data_gen,
            rules_gen: self.rules_gen,
            search_gen: self.search_gen,
            len: self.entries.len(),
            rules: self.cfg.export_rules,
            errors: self.cfg.export_errors,
            warnings: self.cfg.export_warnings,
            search: self.cfg.export_search,
        };
        if self.export_hits.as_ref().is_none_or(|(k, _)| *k != key) {
            let hits: Vec<usize> = (0..self.entries.len())
                .filter(|&i| self.is_export_hit(i))
                .collect();
            self.export_hits = Some((key, hits));
        }
        &self.export_hits.as_ref().unwrap().1
    }

    /// Newest timestamp in the log (cached).
    pub(in crate::app) fn latest_ts(&mut self) -> Option<String> {
        if self
            .latest_ts
            .as_ref()
            .is_none_or(|(g, _)| *g != self.data_gen)
        {
            let latest = export::latest_timestamp(&self.entries).map(ms_to_iso);
            self.latest_ts = Some((self.data_gen, latest));
        }
        self.latest_ts.as_ref().and_then(|(_, t)| t.clone())
    }

    /// Computes what will be exported: highlighted rows → time filter → context ranges
    /// merged → older context clipped → only the last N blocks.
    pub(in crate::app) fn export_plan(&mut self) -> ExportPlan {
        let (before, after, len) = (
            self.cfg.export_before,
            self.cfg.export_after,
            self.entries.len(),
        );
        let since = if self.cfg.export_since_enabled {
            match export::parse_datetime_input(&self.cfg.export_since, &self.date_format) {
                Some(iso) => Some(iso),
                None => {
                    return ExportPlan {
                        hits: Vec::new(),
                        blocks: Vec::new(),
                        since_invalid: true,
                    };
                }
            }
        } else {
            None
        };
        // "Rows visible in the table": every row shown in the table is a highlighted row
        // (the criteria are not used); the context still comes from the whole file.
        let all_hits = if self.cfg.export_visible_only {
            self.filtered.clone()
        } else {
            self.export_hits().to_vec()
        };
        let since = since.as_deref().and_then(iso_to_ms);
        let hits = export::filter_hits_since(&all_hits, &self.entries, since);
        let blocks = export::merge_ranges(&hits, before, after, len);
        let blocks = export::clip_blocks_since(&blocks, &self.entries, since);
        let last = self
            .cfg
            .export_last_enabled
            .then_some(self.cfg.export_last_blocks);
        let blocks = export::keep_last_blocks(blocks, last);
        // Count only hits that are really exported.
        let hits = hits
            .into_iter()
            .filter(|h| blocks.iter().any(|&(a, b)| (a..=b).contains(h)))
            .collect();
        ExportPlan {
            hits,
            blocks,
            since_invalid: false,
        }
    }

    /// Date/time formatted for the input field (without milliseconds).
    pub(in crate::app) fn input_dt(&self, iso: &str) -> String {
        let s = self.display_dt(iso);
        match s.rfind('.') {
            Some(i) if s.len() - i == 4 => s[..i].to_owned(),
            _ => s,
        }
    }

    pub(in crate::app) fn export_window(&mut self, ctx: &egui::Context) {
        if !self.show_export {
            return;
        }
        let mut open = true;
        let mut start = false;
        let mut changed = false;
        egui::Window::new(format!("📤 {}", t!("export.title")))
            .id(egui::Id::unique("export_window"))
            .open(&mut open)
            .resizable(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                egui::Grid::new("export_grid")
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(t!("export.before"));
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut self.cfg.export_before)
                                    .range(0..=100_000),
                            )
                            .changed();
                        ui.end_row();
                        ui.label(t!("export.after"));
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut self.cfg.export_after).range(0..=100_000),
                            )
                            .changed();
                        ui.end_row();
                    });
                ui.separator();
                ui.label(t!("export.include"));
                let (visible, total) = (self.filtered.len(), self.entries.len());
                changed |= ui
                    .checkbox(
                        &mut self.cfg.export_visible_only,
                        t!("export.visible_only", visible = visible, total = total),
                    )
                    .on_hover_text(t!("export.visible_only.tip"))
                    .changed();
                // The criteria are not used while every visible row counts as highlighted.
                ui.add_enabled_ui(!self.cfg.export_visible_only, |ui| {
                    changed |= ui
                        .checkbox(&mut self.cfg.export_rules, t!("export.inc.rules"))
                        .changed();
                    changed |= ui
                        .checkbox(&mut self.cfg.export_errors, t!("export.inc.errors"))
                        .changed();
                    changed |= ui
                        .checkbox(&mut self.cfg.export_warnings, t!("export.inc.warnings"))
                        .changed();
                    changed |= ui
                        .add_enabled(
                            self.search_re.is_some(),
                            egui::Checkbox::new(
                                &mut self.cfg.export_search,
                                t!("export.inc.search"),
                            ),
                        )
                        .changed();
                });
                ui.separator();
                ui.label(t!("export.format"));
                changed |= ui
                    .radio_value(
                        &mut self.cfg.export_format,
                        ExportFormat::Original,
                        t!("export.format.original"),
                    )
                    .changed();
                changed |= ui
                    .radio_value(
                        &mut self.cfg.export_format,
                        ExportFormat::Readable,
                        t!("export.format.readable"),
                    )
                    .changed();
                changed |= ui
                    .checkbox(&mut self.cfg.export_separators, t!("export.separators"))
                    .changed();
                ui.separator();

                // --- Only records from a date/time on ---
                let latest = self.latest_ts();
                let example = self.input_dt(latest.as_deref().unwrap_or("2026-10-23 08:00:00.000"));
                ui.horizontal(|ui| {
                    changed |= ui
                        .checkbox(&mut self.cfg.export_since_enabled, t!("export.since"))
                        .changed();
                    changed |= ui
                        .add_enabled(
                            self.cfg.export_since_enabled,
                            egui::TextEdit::singleline(&mut self.cfg.export_since)
                                .hint_text(t!("export.since.hint", example = example))
                                .desired_width(190.0)
                                .font(TextStyle::Monospace),
                        )
                        .changed();
                });
                if self.cfg.export_since_enabled {
                    let mut preset: Option<String> = None;
                    ui.horizontal_wrapped(|ui| {
                        let sel_ts = self
                            .selected
                            .and_then(|i| self.entries.get(i))
                            .and_then(|e| e.timestamp())
                            .map(ms_to_iso);
                        if ui
                            .add_enabled(
                                sel_ts.is_some(),
                                egui::Button::new(t!("export.since.selected")),
                            )
                            .clicked()
                        {
                            preset = sel_ts;
                        }
                        for (hours, key) in [
                            (1, "export.since.last_hour"),
                            (24, "export.since.last_day"),
                            (24 * 7, "export.since.last_week"),
                        ] {
                            if ui
                                .add_enabled(latest.is_some(), egui::Button::new(t!(key)))
                                .on_hover_text(t!("export.since.presets.tip"))
                                .clicked()
                            {
                                preset = latest
                                    .as_deref()
                                    .and_then(|l| export::hours_before(l, hours));
                            }
                        }
                    });
                    if let Some(iso) = preset {
                        self.cfg.export_since = self.input_dt(&iso);
                        changed = true;
                    }
                    if latest.is_none() {
                        ui.colored_label(ORANGE, t!("export.since.no_time"));
                    } else {
                        match export::parse_datetime_input(
                            &self.cfg.export_since,
                            &self.date_format,
                        ) {
                            Some(iso) => {
                                ui.weak(t!("export.since.parsed", time = self.input_dt(&iso)));
                            }
                            None => {
                                ui.colored_label(
                                    RED,
                                    t!("export.since.invalid", example = example),
                                );
                            }
                        }
                    }
                }

                // --- Only the last N blocks ---
                ui.horizontal(|ui| {
                    changed |= ui
                        .checkbox(&mut self.cfg.export_last_enabled, t!("export.last"))
                        .changed();
                    changed |= ui
                        .add_enabled(
                            self.cfg.export_last_enabled,
                            egui::DragValue::new(&mut self.cfg.export_last_blocks)
                                .range(1..=100_000),
                        )
                        .changed();
                    ui.label(t!("export.last.unit"));
                });
                ui.separator();
                ui.weak(t!("export.note"));
                ui.add_space(4.0);

                let plan = self.export_plan();
                let since_invalid = plan.since_invalid;
                let hits = plan.hits.len();
                let blocks = plan.blocks;
                let rows = export::row_count(&blocks);
                ui.strong(t!(
                    "export.summary",
                    hits = hits,
                    blocks = blocks.len(),
                    rows = rows
                ));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let busy = self.export_job.is_some() || self.loading.is_some();
                    if ui
                        .add_enabled(
                            rows > 0 && !busy,
                            egui::Button::new(format!("💾 {}", t!("export.button"))),
                        )
                        .clicked()
                    {
                        start = true;
                    }
                    if self.export_job.is_some() {
                        ui.spinner();
                        ui.label(t!("export.running"));
                    } else if since_invalid {
                        ui.colored_label(RED, t!("export.since.fix"));
                    } else if rows == 0 {
                        ui.weak(t!("export.nothing"));
                    }
                });
            });
        if changed {
            self.cfg_dirty = true;
        }
        if start {
            self.start_export();
        }
        self.show_export = open;
    }

    /// Asks for the target file and writes the export in a background thread.
    pub(in crate::app) fn start_export(&mut self) {
        let Some(src) = self.cur_path.clone() else {
            return;
        };
        let plan = self.export_plan();
        let blocks = plan.blocks;
        let hits = plan.hits;
        if blocks.is_empty() {
            return;
        }
        let readable = self.cfg.export_format == ExportFormat::Readable;
        let stem = src
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "log".into());
        let (ext, filter) = if readable {
            ("txt", t!("dialog.filter.text"))
        } else {
            ("log", t!("dialog.filter.logs"))
        };
        let mut dlg = rfd::FileDialog::new()
            .add_filter(filter, &[ext])
            .set_file_name(format!("{stem}_export.{ext}"));
        if let Some(dir) = src.parent() {
            dlg = dlg.set_directory(dir);
        }
        let Some(target) = dlg.save_file() else {
            return;
        };

        // Blocks split by file (a log merged from rotated parts); file names only then.
        let multi = self.files.len() > 1;
        let segs: Vec<Vec<export::Segment>> = blocks
            .iter()
            .map(|&b| export::segments(&self.entries, b))
            .collect();
        let seg_sep = |s: &export::Segment| {
            let name = multi.then(|| self.file_label(s.file));
            export::separator(name.as_deref(), s.lines.0, s.lines.1)
        };
        let rows = export::row_count(&blocks);
        let separators = self.cfg.export_separators;
        // The readable text is built here (it needs translations and date formatting);
        // the original lines are read from the files in the thread.
        let readable_text = readable.then(|| {
            let block_seps: Vec<String> = segs
                .iter()
                .map(|parts| match parts.as_slice() {
                    [one] => seg_sep(one),
                    many => {
                        let label: Vec<String> = many
                            .iter()
                            .map(|s| {
                                format!(
                                    "{}: {} – {}",
                                    self.file_label(s.file),
                                    s.lines.0,
                                    s.lines.1
                                )
                            })
                            .collect();
                        format!("-------- {} --------", label.join(", "))
                    }
                })
                .collect();
            let row_blocks: Vec<Vec<ReadableRow<'_>>> = blocks
                .iter()
                .map(|&(a, b)| {
                    (a..=b)
                        .map(|i| {
                            let e = self.entries.at(i);
                            ReadableRow {
                                highlighted: hits.binary_search(&i).is_ok(),
                                line: if multi {
                                    format!("{}:{}", self.file_label(e.file), e.line_no)
                                } else {
                                    e.line_no.to_string()
                                },
                                datetime: self.display_dt(&e.datetime()),
                                severity: match e.severity {
                                    Severity::Error => t!("severity.error"),
                                    Severity::Warning => t!("severity.warning"),
                                    Severity::Info => t!("severity.info"),
                                },
                                entry: e,
                            }
                        })
                        .collect()
                })
                .collect();
            // Extra columns of a log definition are appended with their headers.
            let mut header = t!("export.header");
            for (i, name) in self.schema.extra_names.iter().enumerate() {
                let title = self
                    .schema
                    .columns
                    .iter()
                    .find(|c| c.kind == ColumnKind::Extra(i))
                    .and_then(|c| c.title.as_deref())
                    .unwrap_or(name);
                header.push('\t');
                header.push_str(title);
            }
            let extra_cols = self.schema.extra_names.len();
            export::build_readable(&header, &row_blocks, &block_seps, separators, extra_cols)
        });
        // Original lines: every part is read from its file in the thread; a part whose file
        // was overwritten by a later rotation falls back to the message texts.
        enum PartSrc {
            File { job: usize, range: usize },
            Text(Vec<String>),
        }
        let mut jobs: Vec<(PathBuf, Vec<(usize, usize)>)> = Vec::new();
        let mut parts: Vec<(String, PartSrc)> = Vec::new();
        if !readable {
            for s in segs.iter().flatten() {
                let src = match self.files.get(s.file as usize).and_then(|f| f.path.clone()) {
                    Some(path) => {
                        let job = match jobs.iter().position(|(p, _)| *p == path) {
                            Some(j) => j,
                            None => {
                                jobs.push((path, Vec::new()));
                                jobs.len() - 1
                            }
                        };
                        jobs[job].1.push(s.lines);
                        PartSrc::File {
                            job,
                            range: jobs[job].1.len() - 1,
                        }
                    }
                    None => PartSrc::Text(
                        (s.entries.0..=s.entries.1)
                            .flat_map(|i| {
                                self.entries
                                    .message(i)
                                    .lines()
                                    .map(str::to_owned)
                                    .collect::<Vec<_>>()
                            })
                            .collect(),
                    ),
                };
                parts.push((seg_sep(s), src));
            }
        }
        let nblocks = blocks.len();
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        let done_msg = t!(
            "export.done",
            rows = rows,
            blocks = nblocks,
            path = target.display()
        );
        let spawned = std::thread::Builder::new()
            .name("rustrace-export".into())
            .spawn(move || {
                let text = match readable_text {
                    Some(t) => Ok(t),
                    None => jobs
                        .iter()
                        .map(|(path, ranges)| crate::parser::read_line_ranges(path, ranges))
                        .collect::<std::io::Result<Vec<_>>>()
                        .map(|mut read| {
                            let parts: Vec<(String, Vec<String>)> = parts
                                .into_iter()
                                .map(|(sep, src)| match src {
                                    PartSrc::File { job, range } => {
                                        (sep, std::mem::take(&mut read[job][range]))
                                    }
                                    PartSrc::Text(t) => (sep, t),
                                })
                                .collect();
                            export::build_original(&parts, separators)
                        }),
                };
                let res = text
                    .and_then(|t| std::fs::write(&target, t))
                    .map(|_| done_msg)
                    .map_err(|e| e.to_string());
                let _ = tx.send(res);
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.export_job = Some(rx),
            Err(e) => self.status_error = Some(t!("error.thread", error = e)),
        }
    }

    pub(in crate::app) fn check_export(&mut self) {
        let Some(rx) = &self.export_job else { return };
        match rx.try_recv() {
            Ok(Ok(msg)) => {
                self.export_job = None;
                self.status_error = None;
                self.status_info = Some((msg, Instant::now() + Duration::from_secs(5)));
            }
            Ok(Err(e)) => {
                self.export_job = None;
                self.status_error = Some(t!("error.export", error = e));
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.ctx.request_repaint_after(Duration::from_millis(100));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.export_job = None;
                self.status_error = Some(t!("error.export", error = "internal error"));
            }
        }
    }
}
