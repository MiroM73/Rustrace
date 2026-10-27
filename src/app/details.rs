//! The details pane of the selected row and error code explanations.

use crate::app::*;

impl App {
    /// Error codes in the selected row (cached).
    pub(in crate::app) fn selected_codes(&mut self) -> Vec<CodeInfo> {
        let Some(i) = self.details_row() else {
            return Vec::new();
        };
        if let Some((ci, c)) = &self.code_cache
            && *ci == i
        {
            return c.clone();
        }
        let codes: Vec<CodeInfo> = self
            .entries
            .get(i)
            .map(|e| errcodes::find_codes(e.message))
            .unwrap_or_default()
            .into_iter()
            .map(errcodes::lookup)
            .collect();
        self.code_cache = Some((i, codes.clone()));
        codes
    }

    /// Raw line(s) of the selected entry from its file (read once per entry and data change).
    pub(in crate::app) fn selected_raw(&mut self) -> Option<Result<Arc<str>, String>> {
        let i = self.details_row()?;
        if let Some((ci, gen_, r)) = &self.raw_cache
            && *ci == i
            && *gen_ == self.data_gen
        {
            return Some(r.clone());
        }
        let e = self.entries.get(i)?;
        let file = self.files.get(e.file as usize);
        let res = match (
            e.raw_range(),
            file.and_then(|f| Some((f.path.as_ref()?, f.encoding))),
        ) {
            (Some(range), Some((path, enc))) => crate::parser::read_raw(path, range, enc)
                .map(Arc::from)
                .map_err(|e| e.to_string()),
            (Some(_), None) => Err(t!("details.raw.overwritten")),
            (None, _) => Err(t!("details.raw.unknown")),
        };
        self.raw_cache = Some((i, self.data_gen, res.clone()));
        Some(res)
    }

    pub(in crate::app) fn open_lookup(&mut self, code: Option<u32>) {
        if let Some(v) = code {
            self.lookup_input = if v < 0x1_0000 {
                v.to_string()
            } else {
                format!("0x{v:08X}")
            };
        }
        self.show_lookup = true;
        self.lookup_focus = true;
    }

    pub(in crate::app) fn details(&mut self, ui: &mut egui::Ui) {
        let codes = self.selected_codes();
        let raw = match self.cfg.details_view {
            DetailsView::Raw => self.selected_raw(),
            DetailsView::Message => None,
        };
        let mut open_code: Option<u32> = None;
        let mut view = self.cfg.details_view;
        let mut note = self
            .details_row()
            .and_then(|i| self.bookmark_note(i))
            .map(str::to_owned);
        let (mut note_changed, mut toggle_bookmark) = (false, false);
        let Some(e) = self.details_row().and_then(|i| self.entries.get(i)) else {
            ui.centered_and_justified(|ui| {
                ui.weak(t!("details.empty"));
            });
            return;
        };
        let summary = self.selection_summary();
        ui.horizontal_wrapped(|ui| {
            if let Some(s) = summary {
                ui.colored_label(ui.visuals().strong_text_color(), format!("☑ {s}"))
                    .on_hover_text(t!("selection.details.tip"));
                ui.separator();
            }
            // Message | Raw – the same setting as View → Details pane shows.
            ui.selectable_value(&mut view, DetailsView::Message, t!("details.view.message"))
                .on_hover_text(t!("details.view.message.tip"));
            ui.selectable_value(&mut view, DetailsView::Raw, t!("details.view.raw"))
                .on_hover_text(t!("details.view.raw.tip"));
            ui.separator();
            ui.strong(t!("details.line", line = e.line_no));
            // Bookmark of the row with its note (#22).
            let tip = format!("{}   {}", t!("details.bookmark.tip"), kb("Ctrl+F2"));
            if ui
                .selectable_label(note.is_some(), "🔖")
                .on_hover_text(tip)
                .clicked()
            {
                toggle_bookmark = true;
            }
            if let Some(n) = &mut note {
                note_changed = ui
                    .add(
                        egui::TextEdit::singleline(n)
                            .hint_text(t!("bookmarks.note.hint"))
                            .desired_width(220.0),
                    )
                    .changed();
            }
            ui.separator();
            let (txt, col) = match e.severity {
                Severity::Error => (
                    t!("severity.error"),
                    c32(self.cfg.palette(self.dark).error.1),
                ),
                Severity::Warning => (t!("severity.warning"), Color32::from_rgb(200, 160, 0)),
                Severity::Info => (t!("severity.info"), ui.visuals().text_color()),
            };
            ui.colored_label(col, txt);
            // Every column except the message (shown below), in the order of the table.
            for c in &self.columns() {
                let value = match c.kind {
                    ColumnKind::Role(Role::Message) => continue,
                    kind => self.cell_text(&e, kind),
                };
                if value.is_empty() {
                    continue;
                }
                ui.separator();
                let label = match (c.kind, &c.title) {
                    (_, Some(title)) => format!("{title}: {value}"),
                    (ColumnKind::Role(Role::Component), None) => {
                        t!("details.component", component = value)
                    }
                    (ColumnKind::Role(Role::DateTime), None) => t!("details.time", time = value),
                    (ColumnKind::Role(Role::Thread), None) => t!("details.thread", thread = value),
                    _ => value,
                };
                ui.label(label);
            }
        });
        // Explanation of error codes found in the message.
        for c in &codes {
            ui.horizontal(|ui| {
                if ui
                    .small_button(RichText::new(format!("🔎 {}", c.hex())).monospace())
                    .on_hover_text(t!("details.code.open_tip"))
                    .clicked()
                {
                    open_code = Some(c.hresult);
                }
                let col = if c.descriptions.is_empty() {
                    ui.visuals().weak_text_color()
                } else {
                    ui.visuals().strong_text_color()
                };
                let mut tip = format!("{}\n{}", c.forms(), c.facility);
                for (src, d) in &c.descriptions {
                    tip.push_str(&format!("\n\n{src}: {d}"));
                }
                ui.add(egui::Label::new(RichText::new(c.short()).color(col)).truncate())
                    .on_hover_text(tip);
            });
        }
        ui.separator();
        let text: &str = match &raw {
            Some(Ok(r)) => r,
            Some(Err(why)) => {
                ui.weak(t!("details.raw.unavailable", reason = why));
                ""
            }
            None => e.message,
        };
        let font = FontId::monospace(self.cfg.font_size);
        let idx = self.details_row().unwrap_or_default();
        let hit = self.is_match.get(idx);
        // Scroll to the first search match once per selected row, search and view.
        let scroll_key = (idx, self.search_gen, view);
        let first_match = (hit && self.details_scrolled != Some(scroll_key))
            .then(|| self.search_re.as_ref().and_then(|re| re.find(text)))
            .flatten()
            .map(|m| text[..m.start()].chars().count());
        let fg = ui.visuals().text_color();
        let out = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Same highlighting as in the table (text rules + search matches), so a match
                // that does not fit in the table cell is visible here.
                let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                    let text = buf.as_str();
                    let mut job = build_job(text, &self.text_spans(text, hit), fg, &font);
                    job.wrap.max_width = wrap;
                    ui.painter().layout_job(job)
                };
                let mut s = text;
                let te = egui::TextEdit::multiline(&mut s)
                    .desired_width(f32::INFINITY)
                    .font(font.clone())
                    .layouter(&mut layouter)
                    .show(ui);
                if let Some(c) = first_match {
                    let r = te.galley.pos_from_cursor(egui::text::CCursor::new(c));
                    ui.scroll_to_rect(r.translate(te.galley_pos.to_vec2()), Some(Align::Center));
                }
            });
        self.hover_log |= ui.rect_contains_pointer(out.inner_rect);
        if hit {
            self.details_scrolled = Some(scroll_key);
        }
        if view != self.cfg.details_view {
            self.cfg.details_view = view;
            self.cfg_dirty = true;
        }
        if open_code.is_some() {
            self.open_lookup(open_code);
        }
        if let Some(i) = self.details_row() {
            if toggle_bookmark {
                self.toggle_bookmark(i);
            } else if note_changed && let Some(n) = note {
                self.set_bookmark_note(i, n);
            }
        }
    }
}
