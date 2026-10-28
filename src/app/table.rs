//! The log table: row colors, highlighted text and the context menu.

use crate::app::*;

impl App {
    /// Whole-row colors: (foreground, background).
    pub(in crate::app) fn row_colors(&self, e: &EntryRef) -> (Option<Color32>, Option<Color32>) {
        // "Whole row" rules take precedence – the first matching one wins.
        for r in &self.rules {
            if r.rule.scope != HighlightScope::Row {
                continue;
            }
            let hit = r.regex.is_match(e.message)
                || (r.rule.match_component && other_columns_match(&r.regex, e));
            if hit {
                let (fg, bg) = r.rule.colors(self.dark);
                return (
                    r.rule.use_fg.then(|| c32(fg)),
                    r.rule.use_bg.then(|| c32(bg)),
                );
            }
        }
        if self.cfg.color_by_severity {
            let p = self.cfg.palette(self.dark);
            let pair = match e.severity {
                Severity::Error => Some(p.error),
                Severity::Warning => Some(p.warning),
                Severity::Info => None,
            };
            if let Some((fg, bg)) = pair {
                return (Some(c32(fg)), Some(c32(bg)));
            }
        }
        (None, None)
    }

    /// Text of the message cell. Normally the beginning of the first line; when the row matches
    /// the search but the first match in the message would not be visible there (a later line,
    /// past the length cut or past the right edge of the column), an excerpt starting shortly
    /// before the match is shown instead, so the highlighted match is in view.
    pub(in crate::app) fn message_cell_text(
        &self,
        ui: &egui::Ui,
        message: &str,
        hit: bool,
        font: &FontId,
    ) -> String {
        let text = excerpt::first_line(message, MAX_CELL_CHARS);
        let Some(m) = hit
            .then_some(self.search_re.as_ref())
            .flatten()
            .and_then(|re| re.find(message))
        else {
            // No search, or the row matches only in another column.
            return text;
        };
        let width = ui.available_width();
        let fits = |s: &str| {
            ui.painter()
                .layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE)
                .size()
                .x
                <= width
        };
        let first = message.lines().next().unwrap_or("");
        if m.end() <= first.len().min(MAX_CELL_CHARS) && fits(&first[..m.end()]) {
            return text;
        }
        let len = m.end() - m.start();
        let (ex, pos) = excerpt::around(message, m.start(), excerpt::CONTEXT_CHARS, MAX_CELL_CHARS);
        if fits(&ex[..(pos + len).min(ex.len())]) {
            return ex;
        }
        // Narrow column: start right at the match.
        excerpt::around(message, m.start(), 0, MAX_CELL_CHARS).0
    }

    pub(in crate::app) fn text_spans(&self, text: &str, search_hit: bool) -> Vec<Span> {
        let mut spans = Vec::new();
        // Added from the last rule – later spans override earlier ones,
        // so the first rule in the list has the highest priority.
        for r in self.rules.iter().rev() {
            if r.rule.scope != HighlightScope::Text {
                continue;
            }
            let (fg, bg) = r.rule.colors(self.dark);
            for m in r.regex.find_iter(text).take(200) {
                if m.start() < m.end() {
                    spans.push(Span {
                        start: m.start(),
                        end: m.end(),
                        fg: r.rule.use_fg.then(|| c32(fg)),
                        bg: r.rule.use_bg.then(|| c32(bg)),
                    });
                }
            }
        }
        // Search matches are on top.
        if search_hit && let Some(re) = &self.search_re {
            let (sfg, sbg) = self.cfg.palette(self.dark).search;
            for m in re.find_iter(text).take(200) {
                if m.start() < m.end() {
                    spans.push(Span {
                        start: m.start(),
                        end: m.end(),
                        fg: Some(c32(sfg)),
                        bg: Some(c32(sbg)),
                    });
                }
            }
        }
        spans
    }

    pub(in crate::app) fn table(&mut self, ui: &mut egui::Ui) {
        self.hover_log |= ui.rect_contains_pointer(ui.max_rect());
        if self.loading.is_some() && self.entries.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.35);
                ui.set_max_width(480.0);
                self.progress_ui(ui, true);
            });
            return;
        }
        if self.cur_path.is_none() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(t!("table.empty")).size(18.0).weak());
            });
            return;
        }

        let font = FontId::proportional(self.cfg.font_size);
        let row_h = (self.cfg.font_size * 1.45).round() + 2.0;
        let default_fg = ui.visuals().text_color();
        let gap = egui::vec2(
            ui.spacing().item_spacing.x,
            ui.spacing().item_spacing.y * 0.5 + 0.5,
        );

        // Horizontal scrolling (the table itself only scrolls vertically): the table is laid out
        // shifted left by the offset and clipped to the visible area, so its right edge with the
        // vertical scroll bar always stays in view. A horizontal scroll bar below it appears when
        // the columns do not fit (last frame's width); Shift + mouse wheel scrolls as well.
        let parent = ui;
        let full = parent.available_rect_before_wrap();
        // The overview strip (#24) sits right of the table's vertical scroll bar.
        let strip_w = if self.strip_shown() {
            STRIP_WIDTH + 2.0
        } else {
            0.0
        };
        let max_offset = (self.table_width - (full.width() - strip_w)).max(0.0);
        let hbar_h = if max_offset > 0.0 {
            parent.spacing().scroll.bar_width + 4.0
        } else {
            0.0
        };
        let view = egui::Rect::from_min_max(
            full.min,
            egui::pos2(full.max.x - strip_w, full.max.y - hbar_h),
        );
        if parent.rect_contains_pointer(view) {
            self.table_scroll_x -= parent.input(|i| i.smooth_scroll_delta.x);
        }
        self.table_scroll_x = self.table_scroll_x.clamp(0.0, max_offset);
        let mut child =
            parent.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                egui::pos2(view.min.x - self.table_scroll_x, view.min.y),
                view.max,
            )));
        child.set_clip_rect(view.intersect(parent.clip_rect()));
        let ui = &mut child;

        // The columns come from the log format; every layout keeps its own column widths.
        let columns = self.columns();
        let mut tb = TableBuilder::new(ui)
            .id_salt(("log_table", &self.schema.name, columns.len()))
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .stick_to_bottom(self.following())
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::initial(60.0).at_least(40.0).clip(true));
        // With many columns (e.g. IIS) a narrower message leaves room for the others.
        let msg_width = if columns.len() > 5 { 420.0 } else { 720.0 };
        for (i, c) in columns.iter().enumerate() {
            let col = if i + 1 == columns.len() {
                Column::remainder().at_least(LAST_COLUMN_MIN)
            } else {
                match c.kind {
                    ColumnKind::Role(Role::Message) => Column::initial(msg_width).at_least(120.0),
                    ColumnKind::Role(Role::DateTime) => Column::initial(175.0).at_least(40.0),
                    ColumnKind::Delta => Column::initial(100.0).at_least(40.0),
                    _ => Column::initial(150.0).at_least(40.0),
                }
            };
            tb = tb.column(col.clip(true));
        }
        if let Some(r) = self.scroll_to.take() {
            tb = tb.scroll_to_row(r, Some(Align::Center));
        }

        let mut clicked: Option<usize> = None;
        let mut right_clicked: Option<usize> = None;
        let mut action: Option<RowAction> = None;
        let headers: Vec<String> = std::iter::once(t!("column.line"))
            .chain(columns.iter().map(Self::column_title))
            .collect();

        // Filter row under the headers: one field per column of the current log type.
        let show_filters = self.cfg.show_column_filters;
        let mut filter_texts: Vec<String> = columns
            .iter()
            .map(|c| {
                self.col_filter_text
                    .get(&c.kind)
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();
        let mut filters_changed = false;
        let mut clear_filters = false;
        let any_filter = !self.col_filters.is_empty();
        let header_h = if show_filters { 48.0 } else { 22.0 };
        // Left edges of the first and the last header cell → natural width of the table.
        let last_col = headers.len() - 1;
        let mut edges = (0.0f32, 0.0f32);
        // Bottom of the header and the displayed rows drawn (for the overview strip).
        let mut header_bottom = view.min.y;
        let mut drawn: Option<(usize, usize)> = None;
        // Right click on a header: the Δ time column on / off (#23).
        let mut delta_on = self.cfg.delta_column;
        let kinds: Vec<ColumnKind> = columns.iter().map(|c| c.kind).collect();

        // Bottom of the last row when it is drawn: a click below it clears the selection (#37).
        let mut last_row_bottom: Option<f32> = None;
        let n_rows = self.filtered.len();
        let body_out = tb
            .header(header_h, |mut h| {
                for (i, title) in headers.into_iter().enumerate() {
                    let (cell, _) = h.col(|ui| {
                        // Under the widgets of the cell, so the filter field keeps its clicks.
                        let resp = ui.interact(
                            ui.max_rect(),
                            ui.scope_id().with("header"),
                            Sense::click(),
                        );
                        resp.context_menu(|ui| {
                            ui.checkbox(&mut delta_on, t!("delta.column"))
                                .on_hover_text(t!("delta.column.tip"));
                            ui.weak(t!("delta.more_in_view"));
                        });
                        ui.vertical(|ui| {
                            ui.add(
                                egui::Label::new(RichText::new(title).strong()).selectable(false),
                            );
                            if !show_filters {
                                return;
                            }
                            if i == 0 {
                                // The line column: button that clears all column filters.
                                if ui
                                    .add_enabled(any_filter, egui::Button::new("✖").small())
                                    .on_hover_text(t!("filter.clear"))
                                    .clicked()
                                {
                                    clear_filters = true;
                                }
                            } else if kinds[i - 1] == ColumnKind::Delta {
                                // Computed column: nothing to filter.
                            } else {
                                let text = &mut filter_texts[i - 1];
                                let active = ColumnFilter::parse(text).is_some();
                                let mut edit = egui::TextEdit::singleline(text)
                                    .id_salt(("column_filter", i))
                                    .hint_text(t!("filter.hint"))
                                    .desired_width(f32::INFINITY);
                                if active {
                                    edit = edit.background_color(
                                        c32(self.cfg.palette(self.dark).search.1)
                                            .gamma_multiply(0.35),
                                    );
                                }
                                filters_changed |=
                                    ui.add(edit).on_hover_text(t!("filter.tip")).changed();
                            }
                        });
                    });
                    header_bottom = header_bottom.max(cell.bottom());
                    if i == 0 {
                        edges.0 = cell.left();
                    }
                    if i == last_col {
                        edges.1 = cell.left();
                    }
                }
            })
            .body(|body| {
                let st = self.status_columns();
                let status_cols =
                    [st.http, st.substatus, st.win32].map(|c| c.map(ColumnKind::Extra));
                body.rows(row_h, self.filtered.len(), |mut row| {
                    let r = row.index();
                    drawn = Some(drawn.map_or((r, r), |(a, b)| (a.min(r), b.max(r))));
                    let idx = self.filtered[r];
                    let e = self.entries.at(idx);
                    let is_sel = self.sel.get(idx);
                    row.set_selected(is_sel);

                    let (fg, bg) = if is_sel {
                        (None, None)
                    } else {
                        self.row_colors(&e)
                    };
                    let fg_col = fg.unwrap_or(default_fg);

                    let paint_bg = |ui: &mut egui::Ui| {
                        if let Some(bg) = bg {
                            let r = ui.max_rect().expand2(gap);
                            ui.painter().rect_filled(r, 0.0, bg);
                        }
                    };
                    let simple = |ui: &mut egui::Ui, s: &str| {
                        paint_bg(ui);
                        ui.add(
                            egui::Label::new(RichText::new(s).font(font.clone()).color(fg_col))
                                .truncate()
                                .selectable(false),
                        );
                    };

                    // Line number; a bookmarked row has the cell in the bookmark colors (#22).
                    let (_, line_cell) = row.col(|ui| match self.bookmark_note(idx) {
                        Some(_) => {
                            let (bfg, bbg) = self.cfg.palette(self.dark).bookmark;
                            ui.painter()
                                .rect_filled(ui.max_rect().expand2(gap), 0.0, c32(bbg));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("🔖{}", e.line_no))
                                        .font(font.clone())
                                        .color(c32(bfg)),
                                )
                                .truncate()
                                .selectable(false),
                            );
                        }
                        None => simple(ui, &e.line_no.to_string()),
                    });
                    if let Some(note) = self.bookmark_note(idx) {
                        let tip = if note.is_empty() {
                            t!("bookmarks.tip")
                        } else {
                            note.to_owned()
                        };
                        line_cell.on_hover_text(tip);
                    }
                    for c in &columns {
                        match c.kind {
                            ColumnKind::Role(Role::Message) => {
                                row.col(|ui| {
                                    paint_bg(ui);
                                    let hit = self.is_match.get(idx);
                                    let text = self.message_cell_text(ui, e.message, hit, &font);
                                    let spans = self.text_spans(&text, hit);
                                    let job = build_job(&text, &spans, fg_col, &font);
                                    ui.add(egui::Label::new(job).truncate().selectable(false));
                                });
                            }
                            ColumnKind::Role(Role::Component) => {
                                row.col(|ui| simple(ui, e.component));
                            }
                            ColumnKind::Role(Role::Thread) => {
                                row.col(|ui| simple(ui, e.thread));
                            }
                            // A merged view of several logs: a color mark per log.
                            ColumnKind::File if self.cfg.color_files && self.is_merged() => {
                                row.col(|ui| {
                                    paint_bg(ui);
                                    let log = self.file_sources.get(e.file as usize).copied();
                                    let mut job = LayoutJob::default();
                                    job.append(
                                        "■ ",
                                        0.0,
                                        TextFormat::simple(
                                            font.clone(),
                                            log.map_or(fg_col, |l| self.log_color(l)),
                                        ),
                                    );
                                    job.append(
                                        &self.file_label(e.file),
                                        0.0,
                                        TextFormat::simple(font.clone(), fg_col),
                                    );
                                    ui.add(egui::Label::new(job).truncate().selectable(false));
                                });
                            }
                            ColumnKind::Delta => {
                                row.col(|ui| {
                                    let (text, long) = self.delta_cell(idx);
                                    let color = if long {
                                        let (gfg, gbg) = self.cfg.palette(self.dark).gap;
                                        ui.painter().rect_filled(
                                            ui.max_rect().expand2(gap),
                                            0.0,
                                            c32(gbg),
                                        );
                                        c32(gfg)
                                    } else {
                                        paint_bg(ui);
                                        fg_col
                                    };
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(text).font(font.clone()).color(color),
                                        )
                                        .truncate()
                                        .selectable(false),
                                    );
                                });
                            }
                            kind => {
                                let (_, cell) = row.col(|ui| simple(ui, &self.cell_text(&e, kind)));
                                // Status columns of the log definition (#28): explanation as a
                                // tooltip, computed only while the cell is hovered.
                                if status_cols.contains(&Some(kind))
                                    && cell.hovered()
                                    && let Some(tip) = self.status_tip(&e, kind)
                                {
                                    cell.on_hover_text(tip);
                                }
                            }
                        }
                    }

                    let resp = row.response();
                    if r + 1 == n_rows {
                        last_row_bottom = Some(resp.rect.bottom());
                    }
                    if resp.clicked() {
                        clicked = Some(idx);
                    }
                    // Right click on a row outside the selection selects it (like Explorer).
                    if resp.secondary_clicked() && !is_sel {
                        right_clicked = Some(idx);
                    }
                    let multi = self.multi_selected() && is_sel;
                    resp.context_menu(|ui| {
                        if multi {
                            ui.weak(t!("selection.summary", count = self.sel.count()));
                            ui.separator();
                        }
                        let key = if self.is_bookmarked(idx) {
                            "context.bookmark.remove"
                        } else {
                            "context.bookmark.add"
                        };
                        if ui
                            .button(format!("🔖 {}   {}", t!(key), kb("Ctrl+F2")))
                            .clicked()
                        {
                            action = Some(RowAction::ToggleBookmark(idx));
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                e.timestamp().is_some(),
                                egui::Button::new(format!("⏱ {}", t!("context.time_reference"))),
                            )
                            .on_hover_text(t!("context.time_reference.tip"))
                            .clicked()
                        {
                            action = Some(RowAction::SetTimeReference(idx));
                            ui.close();
                        }
                        ui.separator();
                        let copy_key = if multi {
                            "context.copy_messages"
                        } else {
                            "context.copy_message"
                        };
                        if ui
                            .button(format!("{}   {}", t!(copy_key), kb("Ctrl+C")))
                            .clicked()
                        {
                            action = Some(RowAction::CopyMessages);
                            ui.close();
                        }
                        if ui
                            .button(format!(
                                "{}   {}",
                                t!("context.copy_row"),
                                kb("Ctrl+Shift+C")
                            ))
                            .clicked()
                        {
                            action = Some(RowAction::CopyTable);
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                self.copy_job.is_none(),
                                egui::Button::new(t!("context.copy_original")),
                            )
                            .on_hover_text(t!("context.copy_original.tip"))
                            .clicked()
                        {
                            action = Some(RowAction::CopyOriginal);
                            ui.close();
                        }
                        if ui
                            .button(format!("📤 {}", t!("context.export_selected")))
                            .clicked()
                        {
                            action = Some(RowAction::ExportSelected);
                            ui.close();
                        }
                        if ui
                            .button(format!("📊 {}", t!("context.export_selected_table")))
                            .clicked()
                        {
                            action = Some(RowAction::ExportSelectedTable);
                            ui.close();
                        }
                        let codes = errcodes::find_codes(e.message);
                        if !codes.is_empty() {
                            ui.separator();
                            for v in codes {
                                let code = if v < 0x1_0000 {
                                    v.to_string()
                                } else {
                                    format!("0x{v:08X}")
                                };
                                if ui
                                    .button(format!(
                                        "🔎 {}",
                                        t!("context.explain_code", code = code)
                                    ))
                                    .clicked()
                                {
                                    action = Some(RowAction::ExplainCode(v));
                                    ui.close();
                                }
                            }
                        }
                        if !e.component.is_empty() {
                            ui.separator();
                            if ui
                                .button(t!("context.search_component", component = e.component))
                                .clicked()
                            {
                                action = Some(RowAction::SearchComponent(idx));
                                ui.close();
                            }
                            if ui
                                .button(t!("context.highlight_component", component = e.component))
                                .clicked()
                            {
                                action = Some(RowAction::RuleFromComponent(idx));
                                ui.close();
                            }
                        }
                    });
                });
            });

        // "Follow end" paused at a selected row (#37): scrolling the table to the very bottom
        // (mouse wheel, dragging the scroll bar) resumes it.
        if self.follow_paused() {
            let scrollable = body_out.content_size.y > body_out.inner_rect.height() + 1.0;
            let bottom = body_out.content_size.y - body_out.inner_rect.height();
            let at_bottom = body_out.state.clamped_offset().y >= bottom - 2.0;
            let user_scrolled = parent.rect_contains_pointer(view)
                && parent
                    .input(|i| i.smooth_scroll_delta.y < 0.0 || i.pointer.is_decidedly_dragging());
            if scrollable && at_bottom && user_scrolled {
                self.follow.resume();
            }
        }
        // A click into the empty area below the last row clears the selection (#37).
        let empty_click = parent.input(|i| {
            i.pointer
                .primary_clicked()
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        if clicked.is_none()
            && let (Some(pos), Some(bottom)) = (empty_click, last_row_bottom)
            && pos.y > bottom
            && body_out.inner_rect.contains(pos)
            && parent.ctx().layer_id_at(pos) == Some(parent.layer_id())
            && self.selected.is_some()
        {
            self.clear_selection();
        }

        // Natural width: all columns up to the last one plus its minimum width (the last column
        // fills the rest when everything fits).
        self.table_width = edges.1 - edges.0 + LAST_COLUMN_MIN;
        parent.advance_cursor_after_rect(view);
        if strip_w > 0.0 {
            let strip = egui::Rect::from_min_max(
                egui::pos2(view.max.x + 2.0, header_bottom + gap.y * 2.0),
                egui::pos2(full.max.x, view.max.y),
            );
            // Rows only partly in view at the edges do not count.
            let visible = drawn.map(|(a, b)| if b > a + 2 { (a + 1, b - 1) } else { (a, b) });
            self.overview_strip(parent, strip, visible);
        }
        if hbar_h > 0.0 {
            let bar = egui::Rect::from_min_max(egui::pos2(full.min.x, view.max.y), full.max);
            let mut bar_ui = parent.new_child(egui::UiBuilder::new().max_rect(bar));
            bar_ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            let out = egui::ScrollArea::horizontal()
                .id_salt("log_table_hscroll")
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .horizontal_scroll_offset(self.table_scroll_x)
                .show(&mut bar_ui, |ui| {
                    ui.allocate_space(egui::vec2(self.table_width, 1.0));
                });
            self.table_scroll_x = out.state.clamped_offset().x;
        }

        if delta_on != self.cfg.delta_column {
            self.cfg.delta_column = delta_on;
            self.cfg_dirty = true;
        }
        if let Some(i) = clicked {
            let mods = parent.input(|i| i.modifiers);
            self.click_row(i, mods);
        }
        if let Some(i) = right_clicked {
            self.select_only(Some(i));
            self.follow_entry_selected(i);
        }
        if clear_filters {
            self.col_filter_text.clear();
            self.apply_column_filters();
        } else if filters_changed {
            for (c, text) in columns.iter().zip(filter_texts) {
                if text.is_empty() {
                    self.col_filter_text.remove(&c.kind);
                } else {
                    self.col_filter_text.insert(c.kind, text);
                }
            }
            self.apply_column_filters();
        }
        if let Some(a) = action {
            let ctx = ui.ctx().clone();
            match a {
                RowAction::ExplainCode(v) => self.open_lookup(Some(v)),
                RowAction::ToggleBookmark(i) => self.toggle_bookmark(i),
                RowAction::SetTimeReference(i) => self.set_time_reference(i),
                RowAction::CopyMessages => self.copy_messages(&ctx),
                RowAction::CopyTable => self.copy_table(&ctx),
                RowAction::CopyOriginal => self.copy_original(),
                RowAction::ExportSelected => {
                    self.export_selected = true;
                    self.show_export = true;
                }
                RowAction::ExportSelectedTable => {
                    self.open_table_export(Some(crate::tabexport::Scope::Selected))
                }
                RowAction::SearchComponent(i) => {
                    self.search_text = self.entries.at(i).component.to_owned();
                    self.search_confirmed = true;
                    self.search_regex = false;
                    self.recompile_search();
                }
                RowAction::RuleFromComponent(i) => {
                    let comp = self.entries.at(i).component.to_owned();
                    self.cfg.rules.insert(
                        0,
                        HighlightRule {
                            name: t!("rule.component_name", component = comp),
                            pattern: format!("^{}$", regex::escape(&comp)),
                            is_regex: true,
                            match_component: true,
                            ..Default::default()
                        },
                    );
                    self.recompile_rules();
                    self.cfg_dirty = true;
                    self.show_rules = true;
                }
            }
        }
    }
}

/// Builds formatted text with highlighted pieces. Later spans take precedence.
pub(in crate::app) fn build_job(
    text: &str,
    spans: &[Span],
    default_fg: Color32,
    font: &FontId,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    if spans.is_empty() {
        job.append(
            text,
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: default_fg,
                ..Default::default()
            },
        );
        return job;
    }
    let mut cuts: Vec<usize> = Vec::with_capacity(spans.len() * 2 + 2);
    cuts.push(0);
    cuts.push(text.len());
    for s in spans {
        cuts.push(s.start);
        cuts.push(s.end);
    }
    cuts.sort_unstable();
    cuts.dedup();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= b {
            continue;
        }
        let mut fg = default_fg;
        let mut bg = Color32::TRANSPARENT;
        for s in spans {
            if s.start <= a && s.end >= b {
                if let Some(c) = s.fg {
                    fg = c;
                }
                if let Some(c) = s.bg {
                    bg = c;
                }
            }
        }
        job.append(
            &text[a..b],
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: fg,
                background: bg,
                ..Default::default()
            },
        );
    }
    job
}
