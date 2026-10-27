//! Log definitions and the log format menu.

use crate::app::*;

impl App {
    /// (Re)loads the log definitions; a chosen definition is replaced by its new version.
    pub(in crate::app) fn load_defs(&mut self) {
        let set = logdef::load_all(logdef::SourceFilter::from_key(&self.cfg.logdef_sources));
        if let FormatChoice::Def(d) = &self.format_choice {
            self.format_choice = set
                .defs
                .iter()
                .find(|n| n.same_as(d))
                .cloned()
                .map_or(FormatChoice::Auto, FormatChoice::Def);
        }
        self.defs = Arc::new(set.defs);
        self.def_errors = set.errors;
        self.predefined_defs = set.predefined;
        self.own_defs = set.own;
        if !self.def_errors.is_empty() {
            self.status_error = Some(t!("error.logdef", count = self.def_errors.len()));
        }
    }

    /// After an update of RusTrace: new / changed predefined log definitions compared with the
    /// versions the user has seen. The first time the current state is only remembered.
    pub(in crate::app) fn check_def_updates(&mut self) {
        if self.cfg.known_logdefs.is_empty() {
            self.cfg.known_logdefs = self.predefined_defs.clone();
            self.cfg_dirty = true;
            return;
        }
        self.def_updates = logdef::definition_updates(
            &self.cfg.known_logdefs,
            &self.predefined_defs,
            &self.own_defs,
        );
        self.show_def_updates = !self.def_updates.is_empty();
    }

    /// The user has read the notice: the current predefined definitions become the known ones.
    pub(in crate::app) fn acknowledge_def_updates(&mut self) {
        self.cfg.known_logdefs = self.predefined_defs.clone();
        self.cfg_dirty = true;
        self.def_updates.clear();
        self.show_def_updates = false;
    }

    /// Window with the new / changed / removed predefined log definitions.
    pub(in crate::app) fn def_updates_window(&mut self, ctx: &egui::Context) {
        if !self.show_def_updates {
            return;
        }
        let mut open = true;
        let (mut ack, mut close, mut open_folder) = (false, false, false);
        egui::Window::new(format!("🆕 {}", t!("defupd.title")))
            .id(egui::Id::unique("defupd_window"))
            .open(&mut open)
            .default_width(860.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(t!("defupd.intro"));
                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .show(ui, |ui| {
                        for u in &self.def_updates {
                            ui.separator();
                            let head = match u.kind {
                                logdef::UpdateKind::New => {
                                    t!("defupd.new", name = u.name, file = u.id)
                                }
                                logdef::UpdateKind::Changed => {
                                    t!("defupd.changed", name = u.name, file = u.id)
                                }
                                logdef::UpdateKind::Removed => {
                                    t!("defupd.removed", name = u.name, file = u.id)
                                }
                            };
                            ui.strong(head);
                            if let Some(p) = &u.own_path {
                                ui.colored_label(ORANGE, t!("defupd.own_copy", path = p.display()));
                            }
                            if u.changes.is_empty() {
                                continue;
                            }
                            let has_own = u.own_path.is_some();
                            let value =
                                |v: &Option<String>| v.clone().unwrap_or_else(|| "—".to_owned());
                            // Values (patterns can be long) wrap within a fixed width.
                            let cell = |ui: &mut egui::Ui, text: String, width: f32| {
                                ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
                                    ui.add(
                                        egui::Label::new(RichText::new(text).monospace()).wrap(),
                                    );
                                });
                            };
                            egui::Grid::new(("defupd_grid", &u.id))
                                .striped(true)
                                .spacing([14.0, 4.0])
                                .show(ui, |ui| {
                                    ui.strong(t!("defupd.col.key"));
                                    ui.strong(t!("defupd.col.before"));
                                    ui.strong(t!("defupd.col.now"));
                                    if has_own {
                                        ui.strong(t!("defupd.col.own"));
                                    }
                                    ui.end_row();
                                    for c in &u.changes {
                                        ui.monospace(&c.key);
                                        cell(ui, value(&c.old), 250.0);
                                        cell(ui, value(&c.new), 250.0);
                                        if let Some(own) = &c.own {
                                            ui.allocate_ui(egui::vec2(220.0, 0.0), |ui| {
                                                if *own == c.new {
                                                    ui.label(format!(
                                                        "✔ {}",
                                                        t!("defupd.own.done")
                                                    ));
                                                } else if *own == c.old {
                                                    ui.colored_label(
                                                        ORANGE,
                                                        format!("⚠ {}", t!("defupd.own.update")),
                                                    );
                                                } else {
                                                    ui.add(
                                                        egui::Label::new(t!(
                                                            "defupd.own.custom",
                                                            value = value(own)
                                                        ))
                                                        .wrap(),
                                                    );
                                                }
                                            });
                                        }
                                        ui.end_row();
                                    }
                                });
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .button(format!("✔ {}", t!("defupd.ack")))
                        .on_hover_text(t!("defupd.ack.tip"))
                        .clicked()
                    {
                        ack = true;
                    }
                    if ui
                        .button(t!("defupd.later"))
                        .on_hover_text(t!("defupd.later.tip"))
                        .clicked()
                    {
                        close = true;
                    }
                    if ui
                        .button(format!("📂 {}", t!("format.open_folder")))
                        .clicked()
                    {
                        open_folder = true;
                    }
                });
            });
        if open_folder && let Some(dir) = logdef::user_def_dir() {
            let res = std::fs::create_dir_all(&dir).and_then(|_| open::that_detached(&dir));
            if let Err(e) = res {
                self.status_error = Some(t!("error.open_folder", error = e));
            }
        }
        if ack {
            self.acknowledge_def_updates();
        } else if close || !open {
            self.show_def_updates = false;
        }
    }

    /// Changes the format of the current file and reads it again.
    pub(in crate::app) fn set_format(&mut self, choice: FormatChoice) {
        self.format_choice = choice;
        if self.cur_path.is_some() {
            self.reload_full();
        }
    }

    /// Header of a table column.
    pub(in crate::app) fn column_title(c: &LogColumn) -> String {
        if let Some(t) = &c.title {
            return t.clone();
        }
        match c.kind {
            ColumnKind::Role(Role::Message) => t!("column.message"),
            ColumnKind::Role(Role::Component) => t!("column.component"),
            ColumnKind::Role(Role::DateTime) => t!("column.datetime"),
            ColumnKind::Role(Role::Thread) => t!("column.thread"),
            ColumnKind::Extra(_) => String::new(),
            ColumnKind::File => t!("column.file"),
            ColumnKind::Delta => t!("column.delta"),
        }
    }

    /// Name of the format in use.
    /// Name of the format in use, with the file name of the log definition.
    pub(in crate::app) fn format_name(&self) -> String {
        if self.mixed_formats {
            return t!("format.mixed");
        }
        match &self.active_def {
            Some(d) => format!("{} – {}", d.name, d.id),
            None => t!("format.builtin"),
        }
    }

    /// Where a log definition comes from (user folder, folder next to the program, built-in).
    pub(in crate::app) fn def_source(d: &LogDef) -> String {
        if d.is_own() {
            t!("format.source.user")
        } else if d.path.is_some() {
            t!("format.source.exe")
        } else {
            t!("format.source.builtin")
        }
    }

    /// Log definitions whose file name patterns match the open file (when chosen automatically
    /// and more than one matches – the first one is used).
    pub(in crate::app) fn competing_defs(&self) -> Vec<Arc<LogDef>> {
        if self.is_merged() {
            return Vec::new();
        }
        match (&self.cur_path, &self.format_choice) {
            (Some(p), FormatChoice::Auto) => {
                let m = logdef::matching_by_name(&self.defs, p);
                if m.len() > 1 { m } else { Vec::new() }
            }
            _ => Vec::new(),
        }
    }

    /// Tooltip of the format in the status bar: where the definition is loaded from.
    pub(in crate::app) fn format_tip(&self) -> String {
        let mut tip = t!("status.format.tip");
        if let Some(d) = &self.active_def {
            tip.push('\n');
            tip.push_str(
                &d.path
                    .as_ref()
                    .map_or_else(Self::builtin_label, |p| p.display().to_string()),
            );
        }
        let competing = self.competing_defs();
        if !competing.is_empty() {
            tip.push_str("\n\n");
            tip.push_str(&t!("format.several"));
            for d in &competing {
                tip.push_str(&format!(
                    "\n• {} – {} ({})",
                    d.name,
                    d.id,
                    Self::def_source(d)
                ));
            }
        }
        tip
    }

    pub(in crate::app) fn builtin_label() -> String {
        t!("format.source.builtin")
    }

    pub(in crate::app) fn format_menu(&mut self, ui: &mut egui::Ui) {
        let mut choice: Option<FormatChoice> = None;
        let auto_label = if self.cur_path.is_some() {
            t!("format.auto_detected", name = self.format_name())
        } else {
            t!("format.auto")
        };
        if ui
            .radio(matches!(self.format_choice, FormatChoice::Auto), auto_label)
            .on_hover_text(t!("format.auto.tip"))
            .clicked()
        {
            choice = Some(FormatChoice::Auto);
        }
        if ui
            .radio(
                matches!(self.format_choice, FormatChoice::Builtin),
                t!("format.builtin"),
            )
            .clicked()
        {
            choice = Some(FormatChoice::Builtin);
        }
        ui.separator();
        for d in self.defs.iter() {
            let selected = matches!(&self.format_choice, FormatChoice::Def(c) if c.same_as(d));
            let label = format!("{}  ({}, {})", d.name, d.id, Self::def_source(d));
            let mut r = ui.radio(selected, label);
            if let Some(p) = &d.path {
                r = r.on_hover_text(p.display().to_string());
            }
            if r.clicked() {
                choice = Some(FormatChoice::Def(d.clone()));
            }
        }
        // Several definitions match the open file: show which one is used and why.
        let competing = self.competing_defs();
        if !competing.is_empty() {
            ui.separator();
            ui.colored_label(ORANGE, format!("⚠ {}", t!("format.several")))
                .on_hover_text(t!("format.priority.tip"));
            for (i, d) in competing.iter().enumerate() {
                let mark = if i == 0 { "✔" } else { "    " };
                ui.label(format!(
                    "{mark} {} – {} ({})",
                    d.name,
                    d.id,
                    Self::def_source(d)
                ))
                .on_hover_text(
                    d.path
                        .as_ref()
                        .map_or_else(Self::builtin_label, |p| p.display().to_string()),
                );
            }
        }
        ui.separator();
        // Which definitions are used: predefined, own or both.
        ui.separator();
        ui.label(t!("format.sources"));
        let mut sources_changed = false;
        for (filter, key) in [
            (logdef::SourceFilter::All, "format.sources.all"),
            (
                logdef::SourceFilter::Predefined,
                "format.sources.predefined",
            ),
            (logdef::SourceFilter::Own, "format.sources.own"),
        ] {
            let current = logdef::SourceFilter::from_key(&self.cfg.logdef_sources) == filter;
            if ui.radio(current, t!(key)).clicked() && !current {
                self.cfg.logdef_sources = filter.key().to_owned();
                sources_changed = true;
            }
        }
        if sources_changed {
            self.cfg_dirty = true;
            self.load_defs();
            if self.cur_path.is_some() {
                self.reload_full();
            }
            ui.close();
        }
        if ui
            .add_enabled(
                !self.def_updates.is_empty(),
                egui::Button::new(format!("🆕 {}", t!("defupd.menu"))),
            )
            .clicked()
        {
            self.show_def_updates = true;
            ui.close();
        }
        ui.separator();
        if ui.button(format!("🔄 {}", t!("format.reload"))).clicked() {
            self.load_defs();
            if self.def_errors.is_empty() {
                self.info(t!("format.reloaded", count = self.defs.len()));
            }
            if self.cur_path.is_some() {
                self.reload_full();
            }
            ui.close();
        }
        if ui
            .button(format!("📂 {}", t!("format.open_folder")))
            .clicked()
        {
            if let Some(dir) = logdef::user_def_dir() {
                let res = std::fs::create_dir_all(&dir).and_then(|_| open::that_detached(&dir));
                if let Err(e) = res {
                    self.status_error = Some(t!("error.open_folder", error = e));
                }
            }
            ui.close();
        }
        if let Some(dir) = logdef::user_def_dir() {
            ui.weak(dir.display().to_string());
        }
        if !self.def_errors.is_empty() {
            ui.separator();
            ui.colored_label(RED, t!("format.errors"));
            for e in &self.def_errors {
                ui.label(RichText::new(e).small().color(RED));
            }
        }
        if let Some(c) = choice {
            ui.close();
            self.set_format(c);
        }
    }
}
