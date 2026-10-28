//! Help and About windows.

use crate::app::*;

impl App {
    /// Opens a link (a guide) in the default browser / program; on failure the status bar shows
    /// the link so it can be copied (#39).
    pub(in crate::app) fn open_link(&mut self, link: &str) {
        if let Err(e) = open::that_detached(link) {
            self.status_error = Some(t!("error.open_link", link = link, error = e));
        }
    }

    pub(in crate::app) fn help_window(&mut self, ctx: &egui::Context) {
        egui::Window::new(t!("help.title"))
            .id(egui::Id::unique("help_window"))
            .open(&mut self.show_help)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("keys").spacing([20.0, 4.0]).show(ui, |ui| {
                    for (k, d) in [
                        ("Ctrl+O", "help.open"),
                        ("F5", "help.refresh"),
                        ("Ctrl+F5", "help.reload"),
                        ("Ctrl+P", "help.pause"),
                        ("Ctrl+F", "help.search"),
                        ("Enter / F3", "help.next"),
                        ("Shift+Enter / Shift+F3", "help.prev"),
                        ("Ctrl+L", "help.filter"),
                        ("Ctrl+H", "help.rules"),
                        ("Ctrl+E", "help.lookup"),
                        ("Ctrl+F2", "help.bookmark"),
                        ("F2 / Shift+F2", "help.bookmark_next"),
                        ("Ctrl+Shift+S", "help.export"),
                        ("↑ ↓ PgUp PgDn Home", "help.navigate"),
                        ("End", "help.end"),
                        ("Esc", "help.clear_selection"),
                        ("Shift + ↑ ↓ PgUp PgDn", "help.extend"),
                        ("Ctrl / Shift + click", "help.select_click"),
                        ("Ctrl+A", "help.select_all"),
                        ("Ctrl+C", "help.copy"),
                        ("Ctrl+Shift+C", "help.copy_table"),
                        ("Ctrl + / Ctrl −", "help.zoom"),
                    ] {
                        ui.monospace(kb(k));
                        ui.label(t!(d));
                        ui.end_row();
                    }
                    ui.monospace(kb(&format!("Ctrl + {}", t!("key.wheel"))));
                    ui.label(t!("help.zoom_wheel"));
                    ui.end_row();
                    ui.monospace(kb(&format!("Shift + {}", t!("key.wheel"))));
                    ui.label(t!("help.hscroll"));
                    ui.end_row();
                });
                ui.separator();
                ui.label(t!("help.open_hint"));
            });
    }

    pub(in crate::app) fn about_window(&mut self, ctx: &egui::Context) {
        let mut copy = false;
        egui::Window::new(t!("about.title"))
            .id(egui::Id::unique("about_window"))
            .default_width(480.0)
            .open(&mut self.show_about)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading(RichText::new("RusTrace").size(26.0).strong());
                    ui.label(t!("about.version", version = VERSION));
                    ui.add_space(4.0);
                    ui.label(t!("about.description"));
                });
                ui.add_space(8.0);
                egui::Grid::new("about_grid")
                    .num_columns(2)
                    .spacing([16.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(t!("about.author"));
                        ui.hyperlink_to("Miroslav Mišík (MiroM73)", "https://github.com/MiroM73");
                        ui.end_row();
                        ui.label(t!("about.created_by"));
                        ui.add(egui::Label::new(t!("about.created_by.text")).wrap());
                        ui.end_row();
                        ui.label(t!("about.project"));
                        ui.hyperlink(REPOSITORY);
                        ui.end_row();
                        ui.label(t!("about.releases"));
                        ui.hyperlink(format!("{REPOSITORY}/releases"));
                        ui.end_row();
                        ui.label(t!("about.issues"));
                        ui.hyperlink(format!("{REPOSITORY}/issues"));
                        ui.end_row();
                        ui.label(t!("about.guides"));
                        ui.hyperlink_to(
                            t!("about.guides.text"),
                            guides::url(REPOSITORY, guides::INDEX),
                        )
                        .on_hover_text(t!("menu.help.guides.tip"));
                        ui.end_row();
                        ui.label(t!("about.license"));
                        ui.label(t!("about.license.text"));
                        ui.end_row();
                        ui.label(t!("about.cmdline"));
                        ui.add(egui::Label::new(t!("about.cmdline.text")).wrap());
                        ui.end_row();
                        ui.label(t!("about.build"));
                        ui.label(build_info());
                        ui.end_row();
                        if let Some(p) = Config::path() {
                            ui.label(t!("about.settings"));
                            ui.label(p.display().to_string());
                            ui.end_row();
                        }
                    });
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    if ui.button(format!("📋 {}", t!("about.copy"))).clicked() {
                        ui.ctx()
                            .copy_text(format!("{} ({})", base_title(), build_info()));
                        copy = true;
                    }
                });
            });
        if copy {
            self.info(t!("about.copied"));
        }
    }
}
