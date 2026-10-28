//! Export table window (#32, Ctrl+Shift+E): rows, columns, format and its options.

use crate::app::*;
use crate::tabexport::{Col, Scope, TableFormat, csv::CsvEncoding};

impl App {
    pub(in crate::app) fn table_export_window(&mut self, ctx: &egui::Context) {
        if !self.show_table_export {
            return;
        }
        let mut open = true;
        let mut start = false;
        let mut changed = false;
        egui::Window::new(format!("📊 {}", t!("tabexport.title")))
            .id(egui::Id::unique("table_export_window"))
            .open(&mut open)
            .resizable(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                changed |= self.table_export_presets_ui(ui);
                ui.separator();
                changed |= self.table_export_scope_ui(ui);
                ui.separator();
                changed |= self.table_export_columns_ui(ui);
                ui.separator();
                changed |= self.table_export_format_ui(ui);
                ui.separator();
                let scope = self.cfg.table_export.scope;
                let rows = self.table_export_count(scope);
                let cols = self
                    .table_export_columns()
                    .iter()
                    .filter(|(_, on)| *on)
                    .count();
                ui.horizontal(|ui| {
                    let reading = self.loading.as_ref().is_some_and(|l| l.full);
                    let busy = self.table_export_running() || reading;
                    if ui
                        .add_enabled(
                            rows > 0 && cols > 0 && !busy,
                            egui::Button::new(format!("💾 {}", t!("tabexport.button"))),
                        )
                        .clicked()
                    {
                        start = true;
                    }
                    if self.table_export_running() {
                        self.table_export_progress(ui, 260.0);
                    } else if reading {
                        ui.spinner();
                        ui.label(t!("export.reading"));
                    } else if cols == 0 {
                        ui.weak(t!("tabexport.no_columns"));
                    } else if rows == 0 {
                        ui.weak(t!("export.nothing"));
                    } else {
                        ui.weak(t!("tabexport.summary", rows = rows, cols = cols));
                    }
                });
                changed |= ui
                    .checkbox(
                        &mut self.cfg.table_export.open_after,
                        t!("tabexport.open_after"),
                    )
                    .changed();
                if let Some(last) = self.table_export_last.clone() {
                    ui.horizontal_wrapped(|ui| {
                        ui.weak(t!("tabexport.last", path = last.display()));
                        if ui.small_button(t!("tabexport.last.open")).clicked() {
                            self.open_exported(&last, false);
                        }
                        if ui.small_button(t!("tabexport.last.folder")).clicked() {
                            self.open_exported(&last, true);
                        }
                        if ui.small_button(t!("tabexport.last.copy")).clicked() {
                            ui.ctx().copy_text(last.display().to_string());
                        }
                    });
                }
            });
        if changed {
            self.cfg_dirty = true;
        }
        if start {
            self.start_table_export();
        }
        self.show_table_export = open;
    }

    /// Saved settings: choose one, save the current ones under a name, delete one.
    fn table_export_presets_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let mut apply: Option<usize> = None;
        let mut delete: Option<usize> = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(t!("tabexport.preset"));
            let presets = &self.cfg.table_export.presets;
            egui::ComboBox::from_id_salt("tabexport_preset")
                .width(200.0)
                .selected_text(t!("tabexport.preset.choose"))
                .show_ui(ui, |ui| {
                    for (i, (name, _)) in presets.iter().enumerate() {
                        if ui.selectable_label(false, name).clicked() {
                            apply = Some(i);
                        }
                    }
                    if presets.is_empty() {
                        ui.weak(t!("tabexport.preset.none"));
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut self.table_export_preset_name)
                    .hint_text(t!("tabexport.preset.name"))
                    .desired_width(150.0),
            );
            let name = self.table_export_preset_name.trim().to_owned();
            if ui
                .add_enabled(
                    !name.is_empty(),
                    egui::Button::new(format!("💾 {}", t!("tabexport.preset.save"))),
                )
                .on_hover_text(t!("tabexport.preset.save.tip"))
                .clicked()
            {
                self.cfg.table_export.save_preset(&name);
                self.info(t!("tabexport.preset.saved", name = name));
                changed = true;
            }
            if let Some(i) = self
                .cfg
                .table_export
                .presets
                .iter()
                .position(|(n, _)| *n == name)
                && ui
                    .button("🗑")
                    .on_hover_text(t!("tabexport.preset.delete"))
                    .clicked()
            {
                delete = Some(i);
            }
        });
        if let Some(i) = apply {
            let (name, p) = self.cfg.table_export.presets[i].clone();
            self.cfg.table_export.apply_preset(&p);
            // The column choice of a preset belongs to its log types.
            self.table_export_cols = None;
            self.table_export_preset_name = name;
            changed = true;
        }
        if let Some(i) = delete {
            self.cfg.table_export.presets.remove(i);
            changed = true;
        }
        changed
    }

    /// Which rows, with the number of rows of every choice.
    fn table_export_scope_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.label(t!("tabexport.rows"));
        for (scope, key, tip) in [
            (Scope::All, "tabexport.scope.all", "tabexport.scope.all.tip"),
            (
                Scope::Visible,
                "tabexport.scope.visible",
                "tabexport.scope.visible.tip",
            ),
            (
                Scope::Selected,
                "tabexport.scope.selected",
                "tabexport.scope.selected.tip",
            ),
            (
                Scope::Bookmarked,
                "tabexport.scope.bookmarked",
                "tabexport.scope.bookmarked.tip",
            ),
            (
                Scope::Highlighted,
                "tabexport.scope.highlighted",
                "tabexport.scope.highlighted.tip",
            ),
        ] {
            let count = self.table_export_count(scope);
            let enabled = count > 0 || scope == Scope::Visible || scope == Scope::All;
            ui.horizontal(|ui| {
                changed |= ui
                    .add_enabled(
                        enabled,
                        egui::RadioButton::new(
                            self.cfg.table_export.scope == scope,
                            t!(key, count = count),
                        ),
                    )
                    .on_hover_text(t!(tip))
                    .clicked()
                    && {
                        self.cfg.table_export.scope = scope;
                        true
                    };
                if scope == Scope::Highlighted
                    && ui
                        .small_button(format!("⚙ {}", t!("tabexport.scope.highlighted.settings")))
                        .on_hover_text(t!("tabexport.scope.highlighted.settings.tip"))
                        .clicked()
                {
                    self.show_export = true;
                }
            });
        }
        changed
    }

    /// The column checklist with ↑ ↓ to reorder.
    fn table_export_columns_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(t!("tabexport.columns"));
            if ui.small_button(t!("tabexport.columns.all")).clicked() {
                self.table_export_columns()
                    .iter_mut()
                    .for_each(|(_, on)| *on = true);
                changed = true;
            }
            if ui.small_button(t!("tabexport.columns.none")).clicked() {
                self.table_export_columns()
                    .iter_mut()
                    .for_each(|(_, on)| *on = false);
                changed = true;
            }
            if ui
                .small_button(t!("tabexport.columns.reset"))
                .on_hover_text(t!("tabexport.columns.reset.tip"))
                .clicked()
            {
                self.reset_table_export_columns();
            }
        });
        let cols = self.table_export_columns().clone();
        let titles: Vec<String> = cols
            .iter()
            .map(|(c, _)| self.table_export_title(*c))
            .collect();
        let n = cols.len();
        let mut swap: Option<(usize, usize)> = None;
        let mut toggled: Option<usize> = None;
        egui::ScrollArea::vertical()
            .max_height(170.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (i, ((col, on), title)) in cols.iter().zip(&titles).enumerate() {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(i > 0, egui::Button::new("↑").small())
                            .clicked()
                        {
                            swap = Some((i, i - 1));
                        }
                        if ui
                            .add_enabled(i + 1 < n, egui::Button::new("↓").small())
                            .clicked()
                        {
                            swap = Some((i, i + 1));
                        }
                        let mut v = *on;
                        let mut cb = ui.checkbox(&mut v, title);
                        if let Some(tip) = column_tip(*col) {
                            cb = cb.on_hover_text(t!(tip));
                        }
                        if cb.changed() {
                            toggled = Some(i);
                        }
                    });
                }
            });
        if let Some((a, b)) = swap {
            self.table_export_columns().swap(a, b);
            changed = true;
        }
        if let Some(i) = toggled {
            let c = &mut self.table_export_columns()[i];
            c.1 = !c.1;
            changed = true;
        }
        if changed {
            self.save_table_export_columns();
        }
        changed
    }

    /// Format and its options.
    fn table_export_format_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let s = &mut self.cfg.table_export;
        ui.horizontal_wrapped(|ui| {
            ui.label(t!("tabexport.format"));
            for (f, key, tip) in [
                (TableFormat::Csv, "tabexport.format.csv", None),
                (
                    TableFormat::Xlsx,
                    "tabexport.format.xlsx",
                    Some("tabexport.format.xlsx.tip"),
                ),
                (
                    TableFormat::Jsonl,
                    "tabexport.format.jsonl",
                    Some("tabexport.format.jsonl.tip"),
                ),
                (
                    TableFormat::Html,
                    "tabexport.format.html",
                    Some("tabexport.format.html.tip"),
                ),
            ] {
                let mut r = ui.radio_value(&mut s.format, f, t!(key));
                if let Some(tip) = tip {
                    r = r.on_hover_text(t!(tip));
                }
                changed |= r.changed();
            }
        });
        match s.format {
            TableFormat::Csv => {}
            TableFormat::Jsonl => {
                changed |= ui
                    .checkbox(&mut s.severity_english, t!("tabexport.severity_english"))
                    .changed();
                return changed;
            }
            TableFormat::Html => {
                changed |= ui
                    .checkbox(&mut s.xlsx_colors, t!("tabexport.xlsx.colors"))
                    .on_hover_text(t!("tabexport.xlsx.colors.tip"))
                    .changed();
                changed |= ui.checkbox(&mut s.header, t!("tabexport.header")).changed();
                changed |= ui
                    .checkbox(&mut s.severity_english, t!("tabexport.severity_english"))
                    .changed();
                return changed;
            }
            TableFormat::Xlsx => {
                changed |= ui
                    .checkbox(&mut s.xlsx_colors, t!("tabexport.xlsx.colors"))
                    .on_hover_text(t!("tabexport.xlsx.colors.tip"))
                    .changed();
                changed |= ui.checkbox(&mut s.header, t!("tabexport.header")).changed();
                changed |= ui
                    .checkbox(&mut s.severity_english, t!("tabexport.severity_english"))
                    .changed();
                changed |= ui
                    .checkbox(&mut s.xlsx_summary, t!("tabexport.xlsx.summary"))
                    .on_hover_text(t!("tabexport.xlsx.summary.tip"))
                    .changed();
                changed |= ui
                    .checkbox(&mut s.xlsx_statistics, t!("tabexport.xlsx.statistics"))
                    .on_hover_text(t!("tabexport.xlsx.statistics.tip"))
                    .changed();
                changed |= ui
                    .add_enabled(
                        self.files.len() > 1,
                        egui::Checkbox::new(
                            &mut s.xlsx_sheet_per_file,
                            t!("tabexport.xlsx.sheet_per_file"),
                        ),
                    )
                    .on_hover_text(t!("tabexport.xlsx.sheet_per_file.tip"))
                    .changed();
                return changed;
            }
        }
        egui::Grid::new("tabexport_csv")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label(t!("tabexport.separator"));
                let system = crate::tabexport::csv::system_list_separator();
                let name = |c: Option<char>| match c {
                    None => t!("tabexport.separator.system", sep = sep_name(system)),
                    Some(c) => sep_name(c),
                };
                egui::ComboBox::from_id_salt("tabexport_sep")
                    .selected_text(name(s.csv_separator))
                    .show_ui(ui, |ui| {
                        for v in [None, Some(','), Some(';'), Some('\t')] {
                            changed |= ui
                                .selectable_value(&mut s.csv_separator, v, name(v))
                                .changed();
                        }
                    });
                ui.end_row();
                ui.label(t!("tabexport.encoding"));
                let enc_name = |e: CsvEncoding| match e {
                    CsvEncoding::Utf8Bom => t!("tabexport.encoding.utf8bom"),
                    CsvEncoding::Utf8 => t!("tabexport.encoding.utf8"),
                    CsvEncoding::Utf16Le => "UTF-16 LE".to_owned(),
                };
                egui::ComboBox::from_id_salt("tabexport_enc")
                    .selected_text(enc_name(s.csv_encoding))
                    .show_ui(ui, |ui| {
                        for e in [
                            CsvEncoding::Utf8Bom,
                            CsvEncoding::Utf8,
                            CsvEncoding::Utf16Le,
                        ] {
                            changed |= ui
                                .selectable_value(&mut s.csv_encoding, e, enc_name(e))
                                .changed();
                        }
                    });
                ui.end_row();
                ui.label(t!("tabexport.dates"));
                ui.horizontal(|ui| {
                    changed |= ui
                        .radio_value(&mut s.iso_dates, true, t!("tabexport.dates.iso"))
                        .on_hover_text(t!("tabexport.dates.iso.tip"))
                        .changed();
                    changed |= ui
                        .radio_value(&mut s.iso_dates, false, t!("tabexport.dates.ui"))
                        .changed();
                });
                ui.end_row();
            });
        changed |= ui.checkbox(&mut s.header, t!("tabexport.header")).changed();
        changed |= ui
            .checkbox(&mut s.safe_for_excel, t!("tabexport.safe"))
            .on_hover_text(t!("tabexport.safe.tip"))
            .changed();
        changed |= ui
            .checkbox(&mut s.severity_english, t!("tabexport.severity_english"))
            .changed();
        changed
    }
}

/// Tooltip of an additional column.
fn column_tip(col: Col) -> Option<&'static str> {
    Some(match col {
        Col::Delta => "tabexport.col.delta.tip",
        Col::Severity => "tabexport.col.severity.tip",
        Col::Path => "tabexport.col.path.tip",
        Col::Rule => "tabexport.col.rule.tip",
        Col::Note => "tabexport.col.note.tip",
        Col::Codes => "tabexport.col.codes.tip",
        Col::Raw => "tabexport.col.raw.tip",
        Col::Block => "tabexport.col.block.tip",
        _ => return None,
    })
}

fn sep_name(c: char) -> String {
    match c {
        '\t' => t!("tabexport.separator.tab"),
        ';' => format!("{} ( ; )", t!("tabexport.separator.semicolon")),
        ',' => format!("{} ( , )", t!("tabexport.separator.comma")),
        c => c.to_string(),
    }
}
