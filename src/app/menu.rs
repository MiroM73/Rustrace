//! The menu bar and the language menu.

use crate::app::*;

impl App {
    pub(in crate::app) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(t!("menu.file"), |ui| {
                if ui
                    .button(format!("📂 {}   {}", t!("menu.file.open"), kb("Ctrl+O")))
                    .clicked()
                {
                    ui.close();
                    self.open_dialog();
                }
                ui.menu_button(t!("menu.file.recent"), |ui| {
                    if self.cfg.recent_files.is_empty() {
                        ui.weak(t!("menu.file.recent.none"));
                    }
                    let mut chosen = None;
                    for p in &self.cfg.recent_files {
                        if ui.button(p).clicked() {
                            chosen = Some(p.clone());
                        }
                    }
                    if let Some(p) = chosen {
                        ui.close();
                        self.open_file(&p);
                    }
                    if !self.cfg.recent_files.is_empty() {
                        ui.separator();
                        if ui.button(t!("menu.file.recent.clear")).clicked() {
                            self.cfg.recent_files.clear();
                            self.cfg_dirty = true;
                        }
                    }
                });
                ui.separator();
                if ui
                    .button(format!("🔄 {}   F5", t!("menu.file.refresh")))
                    .clicked()
                {
                    ui.close();
                    self.refresh();
                }
                if ui
                    .button(format!("{}   {}", t!("menu.file.reload"), kb("Ctrl+F5")))
                    .clicked()
                {
                    ui.close();
                    self.reload_full();
                }
                ui.separator();
                if ui
                    .add_enabled(
                        self.cur_path.is_some(),
                        egui::Button::new(format!(
                            "📤 {}   {}",
                            t!("menu.file.export"),
                            kb("Ctrl+Shift+S")
                        )),
                    )
                    .clicked()
                {
                    ui.close();
                    self.show_export = true;
                }
                ui.separator();
                if ui.button(t!("menu.file.exit")).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(t!("menu.view"), |ui| {
                if ui
                    .checkbox(&mut self.cfg.show_details, t!("menu.view.details"))
                    .changed()
                {
                    self.cfg_dirty = true;
                }
                if ui
                    .checkbox(&mut self.cfg.auto_scroll, t!("menu.view.follow"))
                    .changed()
                {
                    self.cfg_dirty = true;
                }
                ui.separator();
                ui.label(t!("menu.view.theme"));
                let mut theme_changed = false;
                for (id, key) in [
                    ("system", "menu.view.theme.system"),
                    ("light", "menu.view.theme.light"),
                    ("dark", "menu.view.theme.dark"),
                ] {
                    if ui.radio(self.cfg.theme == id, t!(key)).clicked() {
                        self.cfg.theme = id.to_owned();
                        theme_changed = true;
                    }
                }
                if theme_changed {
                    self.apply_theme(ui.ctx());
                    self.cfg_dirty = true;
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(t!("menu.view.font_size"));
                    if ui
                        .add(
                            egui::Slider::new(&mut self.cfg.font_size, FONT_SIZE_RANGE)
                                .step_by(0.5)
                                .fixed_decimals(1),
                        )
                        .changed()
                    {
                        self.cfg_dirty = true;
                    }
                });
                ui.horizontal(|ui| {
                    let mut changed = ui
                        .checkbox(&mut self.cfg.temp_file_enabled, t!("menu.view.temp_file"))
                        .on_hover_text(t!("menu.view.temp_file.tip"))
                        .changed();
                    changed |= ui
                        .add_enabled(
                            self.cfg.temp_file_enabled,
                            egui::DragValue::new(&mut self.cfg.temp_file_threshold_mb)
                                .range(0..=100_000)
                                .suffix(" MB"),
                        )
                        .on_hover_text(t!("menu.view.temp_file.tip"))
                        .changed();
                    if changed {
                        self.cfg_dirty = true;
                    }
                });
                ui.horizontal(|ui| {
                    let mut changed = ui
                        .checkbox(&mut self.cfg.max_log_enabled, t!("menu.view.max_log"))
                        .on_hover_text(t!("menu.view.max_log.tip"))
                        .changed();
                    changed |= ui
                        .add_enabled(
                            self.cfg.max_log_enabled,
                            egui::DragValue::new(&mut self.cfg.max_log_mb)
                                .range(10..=1_000_000)
                                .suffix(" MB"),
                        )
                        .on_hover_text(t!("menu.view.max_log.tip"))
                        .changed();
                    if changed {
                        self.cfg_dirty = true;
                        self.enforce_memory_limits();
                    }
                });
                if ui
                    .checkbox(&mut self.cfg.merge_rotated, t!("menu.view.merge_rotated"))
                    .on_hover_text(t!("menu.view.merge_rotated.tip"))
                    .changed()
                {
                    self.cfg_dirty = true;
                    if self.cur_path.is_some() {
                        self.reload_full();
                    }
                }
                ui.separator();
                ui.menu_button(format!("📐 {}", t!("menu.view.format")), |ui| {
                    self.format_menu(ui);
                });
                ui.menu_button(format!("🌐 {}", t!("menu.view.language")), |ui| {
                    self.language_menu(ui);
                });
            });
            ui.menu_button(t!("menu.highlight"), |ui| {
                if ui
                    .button(format!(
                        "🎨 {}   {}",
                        t!("menu.highlight.rules"),
                        kb("Ctrl+H")
                    ))
                    .clicked()
                {
                    ui.close();
                    self.show_rules = true;
                }
                if ui.button(t!("menu.highlight.colors")).clicked() {
                    ui.close();
                    self.show_colors = true;
                }
            });
            ui.menu_button(t!("menu.tools"), |ui| {
                if ui
                    .button(format!("🔎 {}   {}", t!("menu.tools.lookup"), kb("Ctrl+E")))
                    .clicked()
                {
                    ui.close();
                    self.open_lookup(None);
                }
            });
            ui.menu_button(t!("menu.help"), |ui| {
                if ui.button(t!("menu.help.shortcuts")).clicked() {
                    ui.close();
                    self.show_help = true;
                }
                #[cfg(windows)]
                if ui
                    .button(format!("🔗 {}", t!("menu.help.default_app")))
                    .clicked()
                {
                    ui.close();
                    self.assoc = Some(fileassoc::info());
                }
                if ui
                    .button(format!("⬆ {}", t!("menu.help.updates")))
                    .clicked()
                {
                    ui.close();
                    self.show_update = true;
                    if !matches!(self.update, UpdateState::Installing { .. }) {
                        self.start_update_check(false);
                    }
                }
                if ui.button(format!("ℹ {}", t!("menu.help.about"))).clicked() {
                    ui.close();
                    self.show_about = true;
                }
                ui.separator();
                if let Some(p) = Config::path() {
                    ui.weak(t!("menu.help.settings_file", path = p.display()));
                }
            });
        });
    }

    pub(in crate::app) fn language_menu(&mut self, ui: &mut egui::Ui) {
        let auto = self.cfg.language.is_empty();
        if ui
            .radio(auto, t!("lang.auto", code = i18n::system_language()))
            .clicked()
        {
            self.set_language("", false);
            ui.close();
        }
        ui.separator();
        for l in i18n::available() {
            let selected = !auto && self.lang_code.eq_ignore_ascii_case(&l.code);
            let mut label = format!("{}  ({})", l.name, l.code);
            if l.external {
                label.push_str(" *");
            }
            if ui.radio(selected, label).clicked() {
                self.set_language(&l.code, false);
                ui.close();
            }
        }
        ui.separator();
        if ui.button(format!("🔄 {}", t!("lang.reload"))).clicked() {
            let code = self.cfg.language.clone();
            self.set_language(&code, true);
            self.info(t!("lang.reloaded"));
            ui.close();
        }
        if ui
            .button(format!("📂 {}", t!("lang.open_folder")))
            .clicked()
        {
            if let Some(dir) = i18n::user_lang_dir() {
                let res = std::fs::create_dir_all(&dir).and_then(|_| open::that_detached(&dir));
                if let Err(e) = res {
                    self.status_error = Some(t!("error.open_folder", error = e));
                }
            }
            ui.close();
        }
        ui.menu_button(format!("➕ {}", t!("lang.new")), |ui| {
            ui.label(t!("lang.new.hint"));
            ui.horizontal(|ui| {
                ui.label(t!("lang.new.code"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_lang_code)
                        .hint_text("fr, pl, hu, pt-BR …")
                        .desired_width(90.0),
                );
            });
            let code = self.new_lang_code.trim().to_owned();
            let valid = !code.is_empty()
                && code.len() <= 12
                && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            if ui
                .add_enabled(valid, egui::Button::new(t!("lang.new.create")))
                .clicked()
            {
                match i18n::user_lang_dir()
                    .ok_or_else(|| std::io::Error::other("no config dir"))
                    .and_then(|dir| i18n::export_template(&dir, &code))
                {
                    Ok(path) => {
                        let _ = open::that_detached(&path);
                        self.info(t!("lang.new.created", path = path.display()));
                        self.new_lang_code.clear();
                        let lang = self.cfg.language.clone();
                        self.set_language(&lang, true);
                    }
                    Err(e) => self.status_error = Some(t!("error.create_lang", error = e)),
                }
                ui.close();
            }
        });
        if let Some(dir) = i18n::user_lang_dir() {
            ui.weak(dir.display().to_string());
        }
    }
}
