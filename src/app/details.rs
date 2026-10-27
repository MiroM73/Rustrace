//! The details pane of the selected row and error code / status code explanations.

use crate::app::*;

/// Codes explained for the selected row: error codes found in the message and the values of
/// the status columns of the log definition (#28).
#[derive(Clone, Default)]
pub(in crate::app) struct RowCodes {
    pub(in crate::app) codes: Vec<CodeInfo>,
    /// HTTP status + IIS substatus (`http_status`, `http_substatus`).
    pub(in crate::app) http: Option<HttpStatus>,
    /// Win32 status (`win32_status`) when it is not 0.
    pub(in crate::app) win32: Option<CodeInfo>,
}

impl App {
    /// Extra column indexes of the status columns of the open log (#28).
    pub(in crate::app) fn status_columns(&self) -> logdef::StatusColumns {
        self.schema.status.resolve(&self.schema.extra_names)
    }

    /// Error codes in the selected row and its status columns (cached).
    pub(in crate::app) fn selected_codes(&mut self) -> RowCodes {
        let Some(i) = self.details_row() else {
            return RowCodes::default();
        };
        if let Some((ci, c)) = &self.code_cache
            && *ci == i
        {
            return c.clone();
        }
        let st = self.status_columns();
        let row = match self.entries.get(i) {
            Some(e) => {
                let win32 = st
                    .win32
                    .and_then(|c| httpstatus::win32_from_cell(e.extra(c)))
                    .filter(|&v| v != 0);
                RowCodes {
                    codes: errcodes::find_codes(e.message)
                        .into_iter()
                        .filter(|&v| Some(v) != win32)
                        .map(errcodes::lookup)
                        .collect(),
                    http: st.http.and_then(|c| {
                        httpstatus::from_cells(e.extra(c), st.substatus.map(|s| e.extra(s)))
                    }),
                    win32: win32.map(errcodes::lookup),
                }
            }
            None => RowCodes::default(),
        };
        self.code_cache = Some((i, row.clone()));
        row
    }

    /// Tooltip of a status cell in the table (#28) – computed only while the cell is hovered.
    pub(in crate::app) fn status_tip(&self, e: &EntryRef, kind: ColumnKind) -> Option<String> {
        let ColumnKind::Extra(c) = kind else {
            return None;
        };
        let st = self.status_columns();
        if st.http.is_some_and(|h| h == c) || st.substatus.is_some_and(|s| s == c) {
            let h = httpstatus::from_cells(e.extra(st.http?), st.substatus.map(|s| e.extra(s)))?;
            return Some(h.short());
        }
        if st.win32 == Some(c) {
            let v = httpstatus::win32_from_cell(e.extra(c))?;
            if v == 0 {
                return Some(t!("code.success"));
            }
            let info = errcodes::lookup(v);
            return Some(format!("Win32 {v}: {}\n{}", info.short(), info.forms()));
        }
        None
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

    /// Opens the error code lookup with a text, e.g. "HTTP 404.3".
    pub(in crate::app) fn open_lookup_text(&mut self, text: String) {
        self.lookup_input = text;
        self.show_lookup = true;
        self.lookup_focus = true;
    }

    pub(in crate::app) fn details(&mut self, ui: &mut egui::Ui) {
        let RowCodes { codes, http, win32 } = self.selected_codes();
        let mut open_text: Option<String> = None;
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
                    // The Δ values follow below.
                    ColumnKind::Role(Role::Message) | ColumnKind::Delta => continue,
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
            // Time to the previous row and to the reference row (#23).
            let idx = self.details_row().unwrap_or_default();
            if let Some(d) = self.delta_ms(idx, DeltaMode::Visible) {
                ui.separator();
                ui.label(t!(
                    "details.delta.previous",
                    delta = crate::timefmt::format_delta(d)
                ))
                .on_hover_text(t!("delta.mode.visible"));
            }
            if let Some(d) = self.delta_ms(idx, DeltaMode::Reference) {
                ui.separator();
                ui.label(t!(
                    "details.delta.reference",
                    delta = crate::timefmt::format_delta(d)
                ));
            }
        });
        // HTTP status of the status columns (#28).
        if let Some(h) = &http {
            ui.horizontal(|ui| {
                if ui
                    .small_button(RichText::new(format!("🔎 HTTP {}", h.code())).monospace())
                    .on_hover_text(t!("details.code.open_tip"))
                    .clicked()
                {
                    open_text = Some(format!("HTTP {}", h.code()));
                }
                let col = if h.exact {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.add(egui::Label::new(RichText::new(h.short()).color(col)).truncate())
                    .on_hover_text(h.short());
            });
        }
        // Win32 status column, then error codes found in the message.
        for (c, is_status) in win32
            .iter()
            .map(|c| (c, true))
            .chain(codes.iter().map(|c| (c, false)))
        {
            ui.horizontal(|ui| {
                let label = match c.win32_code() {
                    Some(w) if is_status => format!("🔎 Win32 {w}"),
                    _ => format!("🔎 {}", c.hex()),
                };
                if ui
                    .small_button(RichText::new(label).monospace())
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
        } else if let Some(text) = open_text {
            self.open_lookup_text(text);
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
