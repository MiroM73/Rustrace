//! Bookmarks window (#22): every bookmark with its line, time, note and message.

use crate::app::*;

impl App {
    pub(in crate::app) fn bookmarks_window(&mut self, ctx: &egui::Context) {
        let mut jump = None;
        let mut remove = None;
        let mut notes: Vec<(usize, String)> = Vec::new();
        let mut clear = false;
        let multi = self.files.len() > 1;
        let spec = ToolSpec::new(
            format!("🔖 {}", t!("bookmarks.title")),
            [760.0, 320.0],
            [420.0, 160.0],
        );
        self.show_tool(
            ctx,
            |a| &mut a.bookmarks_win,
            spec,
            |app, ui, _keep| {
                if app.bookmark_rows.is_empty() {
                    ui.weak(t!("bookmarks.empty", key = kb("Ctrl+F2")));
                    return;
                }
                ui.horizontal(|ui| {
                    ui.label(t!("bookmarks.count", count = app.bookmark_rows.len()));
                    ui.weak(t!("bookmarks.hint"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(format!("🗑 {}", t!("bookmarks.clear"))).clicked() {
                            clear = true;
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("bookmarks_grid")
                            .striped(true)
                            .num_columns(5)
                            .spacing([10.0, 4.0])
                            .show(ui, |ui| {
                                ui.strong("");
                                ui.strong(t!("column.line"));
                                ui.strong(t!("column.datetime"));
                                ui.strong(t!("bookmarks.note"));
                                ui.strong(t!("column.message"));
                                ui.end_row();
                                for &i in &app.bookmark_rows {
                                    let e = app.entries.at(i);
                                    if ui
                                        .small_button("✖")
                                        .on_hover_text(t!("bookmarks.remove"))
                                        .clicked()
                                    {
                                        remove = Some(i);
                                    }
                                    let line = if multi {
                                        format!("{}:{}", app.file_label(e.file), e.line_no)
                                    } else {
                                        e.line_no.to_string()
                                    };
                                    let click = |ui: &mut egui::Ui, text: String, cut: bool| {
                                        let mut label = egui::Label::new(text)
                                            .selectable(false)
                                            .sense(Sense::click());
                                        if cut {
                                            label = label.truncate();
                                        }
                                        ui.add(label).double_clicked()
                                    };
                                    let mut go = click(ui, line, false);
                                    go |= click(ui, app.display_dt(&e.datetime()), false);
                                    let mut note = app
                                        .bookmarks
                                        .get(&(e.file, e.line_no))
                                        .cloned()
                                        .unwrap_or_default();
                                    let edit = egui::TextEdit::singleline(&mut note)
                                        .hint_text(t!("bookmarks.note.hint"));
                                    if ui
                                        .add_sized([200.0, ui.spacing().interact_size.y], edit)
                                        .changed()
                                    {
                                        notes.push((i, note));
                                    }
                                    ui.scope(|ui| {
                                        ui.set_max_width(420.0);
                                        go |= click(ui, excerpt::first_line(e.message, 200), true);
                                    });
                                    if go {
                                        jump = Some(i);
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            },
        );
        for (i, n) in notes {
            self.set_bookmark_note(i, n);
        }
        if let Some(i) = remove {
            self.remove_bookmark(i);
        }
        if clear {
            self.clear_bookmarks();
        }
        if let Some(i) = jump {
            self.jump_to_entry(i);
        }
    }
}
