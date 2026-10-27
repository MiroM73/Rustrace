//! The details pane of the selected row and error code explanations.

use crate::app::*;

impl App {
    /// Error codes in the selected row (cached).
    pub(in crate::app) fn selected_codes(&mut self) -> Vec<CodeInfo> {
        let Some(i) = self.selected else {
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
        let mut open_code: Option<u32> = None;
        let Some(e) = self.selected.and_then(|i| self.entries.get(i)) else {
            ui.centered_and_justified(|ui| {
                ui.weak(t!("details.empty"));
            });
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong(t!("details.line", line = e.line_no));
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
        let font = FontId::monospace(self.cfg.font_size);
        let idx = self.selected.unwrap_or_default();
        let hit = self.is_match.get(idx).copied().unwrap_or(false);
        // Scroll to the first search match once per selected row and search.
        let scroll_key = (idx, self.search_gen);
        let first_match = (hit && self.details_scrolled != Some(scroll_key))
            .then(|| self.search_re.as_ref().and_then(|re| re.find(e.message)))
            .flatten()
            .map(|m| e.message[..m.start()].chars().count());
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
                let mut s = e.message;
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
        if open_code.is_some() {
            self.open_lookup(open_code);
        }
    }
}
