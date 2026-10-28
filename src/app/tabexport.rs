//! Table export (#32): the rows of the chosen scope with the chosen columns, written as CSV …
//! by `crate::tabexport` in a background thread. The dialog is `dialogs/tabexport.rs`.

use crate::app::*;
use crate::tabexport::{self, Col, ColumnSpec, Control, DeltaFrom, Rows, Scope, TableFormat};
use std::io::Write as _;

/// A table export running in a background thread.
pub(in crate::app) struct TableExportJob {
    /// (rows written, cells cut at the Excel limit), or the error (`None` = cancelled).
    rx: mpsc::Receiver<Result<(usize, usize), Option<String>>>,
    pub(in crate::app) ctl: Control,
    pub(in crate::app) total: usize,
    target: PathBuf,
}

/// The column choice shown in the dialog, for one log type.
pub(in crate::app) struct ColumnChoice {
    /// Log type (`Schema::name`) and its extra column names the choice was made for.
    key: (String, Vec<String>, bool, bool),
    pub(in crate::app) cols: Vec<(Col, bool)>,
}

impl App {
    /// The table columns (line number first) as export columns.
    fn table_export_table_cols(&self) -> Vec<Col> {
        std::iter::once(Col::Line)
            .chain(self.columns().iter().map(|c| match c.kind {
                ColumnKind::Role(Role::Message) => Col::Message,
                ColumnKind::Role(Role::Component) => Col::Component,
                ColumnKind::Role(Role::DateTime) => Col::DateTime,
                ColumnKind::Role(Role::Thread) => Col::Thread,
                ColumnKind::Extra(i) => Col::Extra(i),
                ColumnKind::File => Col::File,
                ColumnKind::Delta => Col::Delta,
            }))
            .collect()
    }

    /// The column choice of the open log type (made from the saved one when the log type or
    /// the table columns changed).
    pub(in crate::app) fn table_export_columns(&mut self) -> &mut Vec<(Col, bool)> {
        let key = (
            self.schema.name.clone(),
            self.schema.extra_names.clone(),
            self.files.len() > 1,
            self.cfg.delta_column,
        );
        if self.table_export_cols.as_ref().is_none_or(|c| c.key != key) {
            let saved = self.cfg.table_export.columns.get(&self.schema.name);
            let cols = tabexport::column_choice(
                &self.table_export_table_cols(),
                self.schema.extra_names.len(),
                &self.schema.extra_names,
                saved.map(Vec::as_slice),
            );
            self.table_export_cols = Some(ColumnChoice { key, cols });
        }
        &mut self.table_export_cols.as_mut().unwrap().cols
    }

    /// Stores the column choice of the open log type in the settings.
    pub(in crate::app) fn save_table_export_columns(&mut self) {
        let names = self.schema.extra_names.clone();
        let saved: Vec<(String, bool)> = self
            .table_export_columns()
            .iter()
            .map(|(c, on)| (c.key(&names), *on))
            .collect();
        let name = self.schema.name.clone();
        self.cfg.table_export.columns.insert(name, saved);
        self.cfg_dirty = true;
    }

    /// Forgets the saved choice: the table columns again.
    pub(in crate::app) fn reset_table_export_columns(&mut self) {
        self.cfg.table_export.columns.remove(&self.schema.name);
        self.table_export_cols = None;
        self.cfg_dirty = true;
    }

    /// Header text of an export column.
    pub(in crate::app) fn table_export_title(&self, col: Col) -> String {
        match col {
            Col::Line => t!("column.line"),
            Col::File => t!("column.file"),
            Col::Path => t!("tabexport.col.path"),
            Col::DateTime => t!("column.datetime"),
            Col::Delta => t!("tabexport.col.delta"),
            Col::Severity => t!("tabexport.col.severity"),
            Col::Rule => t!("tabexport.col.rule"),
            Col::Note => t!("tabexport.col.note"),
            Col::Codes => t!("tabexport.col.codes"),
            Col::Raw => t!("tabexport.col.raw"),
            Col::Block => t!("tabexport.col.block"),
            Col::Message | Col::Component | Col::Thread | Col::Extra(_) => {
                let kind = match col {
                    Col::Message => ColumnKind::Role(Role::Message),
                    Col::Component => ColumnKind::Role(Role::Component),
                    Col::Thread => ColumnKind::Role(Role::Thread),
                    Col::Extra(i) => ColumnKind::Extra(i),
                    _ => unreachable!(),
                };
                match self.schema.columns.iter().find(|c| c.kind == kind) {
                    Some(c) => Self::column_title(c),
                    None => match col {
                        Col::Extra(i) => {
                            self.schema.extra_names.get(i).cloned().unwrap_or_default()
                        }
                        _ => Self::column_title(&LogColumn { kind, title: None }),
                    },
                }
            }
        }
    }

    /// The rows of a scope.
    pub(in crate::app) fn table_export_rows(&mut self, scope: Scope) -> Rows {
        match scope {
            Scope::All => Rows::All(self.entries.len()),
            Scope::Visible => Rows::List(self.filtered.clone()),
            Scope::Selected => Rows::List(self.sel.ones().collect()),
            Scope::Bookmarked => Rows::List(self.bookmark_rows.clone()),
            Scope::Highlighted => {
                // The plan of "Export highlighted rows with context" with its own criteria.
                let sel = std::mem::take(&mut self.export_selected);
                let plan = self.export_plan();
                self.export_selected = sel;
                Rows::Blocks(plan.blocks)
            }
        }
    }

    /// Number of rows of a scope (shown next to it in the dialog).
    pub(in crate::app) fn table_export_count(&mut self, scope: Scope) -> usize {
        match scope {
            Scope::All => self.entries.len(),
            Scope::Visible => self.filtered.len(),
            Scope::Selected => self.sel.count(),
            Scope::Bookmarked => self.bookmark_rows.len(),
            Scope::Highlighted => self.table_export_rows(scope).len(),
        }
    }

    /// Opens the dialog, optionally with a scope (row context menu → selected rows).
    pub(in crate::app) fn open_table_export(&mut self, scope: Option<Scope>) {
        if let Some(s) = scope {
            self.cfg.table_export.scope = s;
            self.cfg_dirty = true;
        }
        self.show_table_export = true;
    }

    pub(in crate::app) fn table_export_running(&self) -> bool {
        self.table_export_job.is_some()
    }

    /// Everything the cells are made of, for the export thread.
    fn table_export_context(&self) -> tabexport::Context {
        let s = &self.cfg.table_export;
        tabexport::Context {
            store: self.entries.clone(),
            files: (0..self.files.len())
                .map(|i| tabexport::FileInfo {
                    label: self.file_label(i as u16),
                    path: self.files[i].path.clone(),
                    encoding: self.files[i].encoding,
                })
                .collect(),
            rules: self
                .rules
                .iter()
                .map(|r| {
                    // The colors of the light theme (the background of a spreadsheet is white).
                    let (fg, bg) = r.rule.colors(false);
                    tabexport::RuleInfo {
                        name: r.rule.name.clone(),
                        regex: r.regex.clone(),
                        row: r.rule.scope == HighlightScope::Row,
                        match_component: r.rule.match_component,
                        fg: r.rule.use_fg.then_some(fg),
                        bg: r.rule.use_bg.then_some(bg),
                    }
                })
                .collect(),
            notes: self.bookmarks.clone(),
            severity_names: if s.severity_english {
                ["Info".into(), "Warning".into(), "Error".into()]
            } else {
                [
                    t!("severity.info"),
                    t!("severity.warning"),
                    t!("severity.error"),
                ]
            },
            date_layout: (!s.iso_dates).then(|| self.date_layout.clone()),
            delta: match self.cfg.delta_mode {
                DeltaMode::Visible => DeltaFrom::Previous,
                DeltaMode::Log => DeltaFrom::Log,
                DeltaMode::Reference => self
                    .time_ref
                    .map_or(DeltaFrom::Previous, |(r, _, _)| DeltaFrom::Reference(r)),
            },
            severity_colors: if self.cfg.color_by_severity {
                let p = self.cfg.palette(false);
                [Some(p.warning), Some(p.error)]
            } else {
                [None, None]
            },
        }
    }

    /// Asks for the target file and writes the export in a background thread.
    pub(in crate::app) fn start_table_export(&mut self) {
        if self.table_export_running() {
            return;
        }
        let scope = self.cfg.table_export.scope;
        let rows = self.table_export_rows(scope);
        let cols: Vec<ColumnSpec> = self
            .table_export_columns()
            .clone()
            .into_iter()
            .filter(|(_, on)| *on)
            .map(|(col, _)| ColumnSpec {
                col,
                title: self.table_export_title(col),
            })
            .collect();
        if rows.is_empty() || cols.is_empty() {
            return;
        }
        let format = self.cfg.table_export.format;
        let ext = format.extension();
        let stem = self
            .cur_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "log".into());
        let filter = match format {
            TableFormat::Csv => t!("tabexport.filter.csv"),
            TableFormat::Xlsx => t!("tabexport.filter.xlsx"),
        };
        let mut dlg = rfd::FileDialog::new()
            .add_filter(filter, &[ext])
            .set_file_name(format!("{stem}_export.{ext}"));
        if let Some(dir) = self.cur_path.as_ref().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(dir);
        }
        let Some(target) = dlg.save_file() else {
            return;
        };
        let ctx = self.table_export_context();
        let s = self.cfg.table_export.clone();
        let opts = tabexport::csv::CsvOptions::new(
            s.csv_separator
                .unwrap_or_else(tabexport::csv::system_list_separator),
            s.csv_encoding,
            s.safe_for_excel,
        );
        let xlsx_opts = tabexport::xlsx::XlsxOptions {
            colors: s.xlsx_colors,
            sheet_name: t!("tabexport.sheet.log"),
            ..Default::default()
        };
        let ctl = Control::default();
        let total = rows.len();
        let (tx, rx) = mpsc::channel();
        let repaint = self.ctx.clone();
        let (thread_ctl, path) = (ctl.clone(), target.clone());
        let spawned = std::thread::Builder::new()
            .name("rustrace-tabexport".into())
            .spawn(move || {
                let res = (|| -> std::io::Result<(usize, usize)> {
                    let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
                    match format {
                        TableFormat::Csv => {
                            let mut w = tabexport::csv::CsvWriter::new(file, opts)?;
                            let n = tabexport::write_table(
                                &ctx,
                                &rows,
                                &cols,
                                s.header,
                                &mut w,
                                &thread_ctl,
                            )?;
                            w.finish()?;
                            Ok((n, 0))
                        }
                        TableFormat::Xlsx => {
                            let mut w = tabexport::xlsx::XlsxWriter::new(file, xlsx_opts);
                            let n = tabexport::write_table(
                                &ctx,
                                &rows,
                                &cols,
                                s.header,
                                &mut w,
                                &thread_ctl,
                            )?;
                            let cut = w.truncated;
                            w.finish()?.flush()?;
                            Ok((n, cut))
                        }
                    }
                })();
                let res = res.map_err(|e| {
                    // A cancelled or failed export leaves no half-written file.
                    let _ = std::fs::remove_file(&path);
                    (e.kind() != std::io::ErrorKind::Interrupted).then(|| e.to_string())
                });
                let _ = tx.send(res);
                repaint.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.table_export_job = Some(TableExportJob {
                    rx,
                    ctl,
                    total,
                    target,
                })
            }
            Err(e) => self.status_error = Some(t!("error.thread", error = e)),
        }
    }

    pub(in crate::app) fn cancel_table_export(&mut self) {
        if let Some(j) = &self.table_export_job {
            j.ctl.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Takes over the result of the export thread.
    pub(in crate::app) fn check_table_export(&mut self) {
        let Some(job) = &self.table_export_job else {
            return;
        };
        match job.rx.try_recv() {
            Ok(res) => {
                let target = job.target.clone();
                self.table_export_job = None;
                match res {
                    Ok((rows, cut)) => {
                        self.status_error = None;
                        let msg = if cut > 0 {
                            t!(
                                "tabexport.done.truncated",
                                rows = rows,
                                path = target.display(),
                                cells = cut
                            )
                        } else {
                            t!("tabexport.done", rows = rows, path = target.display())
                        };
                        self.status_info = Some((msg, Instant::now() + Duration::from_secs(5)));
                    }
                    Err(None) => self.info(t!("tabexport.cancelled")),
                    Err(Some(e)) => self.status_error = Some(t!("error.export", error = e)),
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.ctx.request_repaint_after(Duration::from_millis(100));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.table_export_job = None;
                self.status_error = Some(t!("error.export", error = "internal error"));
            }
        }
    }

    /// Progress of a running export with a cancel button (dialog and status bar).
    pub(in crate::app) fn table_export_progress(&mut self, ui: &mut egui::Ui, width: f32) {
        let Some(job) = &self.table_export_job else {
            return;
        };
        let done = job.ctl.done.load(Ordering::Relaxed).min(job.total);
        let frac = done as f32 / job.total.max(1) as f32;
        ui.spinner();
        ui.add(
            egui::ProgressBar::new(frac)
                .text(t!(
                    "tabexport.progress",
                    done = done,
                    total = job.total,
                    percent = format!("{:.0}", frac * 100.0)
                ))
                .desired_width(width),
        );
        if ui.small_button(t!("progress.cancel")).clicked() {
            self.cancel_table_export();
        }
    }
}
