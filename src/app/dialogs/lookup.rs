//! Error code lookup window (Ctrl+E) – error codes and HTTP status codes (#28).

use crate::app::*;

impl App {
    pub(in crate::app) fn lookup_window(&mut self, ctx: &egui::Context) {
        let spec = ToolSpec::new(
            format!("🔎 {}", t!("lookup.title")),
            [560.0, 420.0],
            [380.0, 200.0],
        );
        self.show_tool(
            ctx,
            |a| &mut a.lookup_win,
            spec,
            |app, ui, _keep| {
                ui.horizontal(|ui| {
                    ui.label(t!("lookup.code"));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut app.lookup_input)
                            .hint_text("0x80070005, -2147024891, 1603, 404.3, HTTP 500 …")
                            .desired_width(280.0)
                            .font(TextStyle::Monospace),
                    );
                    if app.lookup_focus {
                        r.request_focus();
                        app.lookup_focus = false;
                    }
                    if !app.lookup_input.is_empty() && ui.small_button("✖").clicked() {
                        app.lookup_input.clear();
                    }
                });
                ui.separator();

                let input = app.lookup_input.trim().to_owned();
                if input.is_empty() {
                    ui.weak(t!("lookup.hint"));
                    return;
                }
                if app
                    .lookup_cache
                    .as_ref()
                    .is_none_or(|(k, _, _)| *k != input)
                {
                    // `404.3` / `HTTP 500` = HTTP only, plain `404` = both (#28).
                    let (http, code) = httpstatus::lookup_input(&input);
                    app.lookup_cache = Some((
                        input.clone(),
                        code.map(errcodes::lookup),
                        http.map(|q| httpstatus::describe(q.status, q.substatus)),
                    ));
                }
                let Some((_, code, http)) = &app.lookup_cache else {
                    return;
                };
                match (code, http) {
                    (None, None) => {
                        ui.colored_label(RED, t!("lookup.invalid"));
                    }
                    (None, Some(h)) => http_section(ui, h),
                    (Some(info), h) => {
                        code_section(ui, info);
                        if let Some(h) = h {
                            ui.separator();
                            ui.weak(t!("lookup.also_http", code = h.code()));
                            http_section(ui, h);
                        }
                    }
                }
            },
        );
    }
}

/// Explanation of an error code (HRESULT / Win32 / MSI).
fn code_section(ui: &mut egui::Ui, info: &CodeInfo) {
    egui::Grid::new("code_grid")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label(t!("lookup.hex"));
            ui.monospace(info.hex());
            ui.end_row();
            ui.label(t!("lookup.decimal"));
            ui.monospace(t!(
                "lookup.decimal.value",
                signed = info.signed(),
                unsigned = info.hresult
            ));
            ui.end_row();
            if let Some(w) = info.win32_code() {
                ui.label(t!("lookup.win32"));
                ui.monospace(format!("{w}  (0x{w:X})"));
                ui.end_row();
            }
            ui.label(t!("lookup.facility"));
            ui.label(&info.facility);
            ui.end_row();
        });
    ui.separator();
    if info.descriptions.is_empty() {
        ui.weak(t!("lookup.no_description"));
        if !cfg!(windows) {
            ui.weak(t!("lookup.windows_only"));
        }
    }
    for (src, d) in &info.descriptions {
        ui.strong(src);
        ui.label(d);
        ui.add_space(4.0);
    }
    if let Some((_, d)) = info.descriptions.first()
        && ui.button(format!("📋 {}", t!("lookup.copy"))).clicked()
    {
        ui.ctx().copy_text(format!("{} – {d}", info.hex()));
    }
}

/// Explanation of an HTTP status with its IIS substatus (#28).
fn http_section(ui: &mut egui::Ui, h: &HttpStatus) {
    ui.strong(t!("lookup.http.title", code = h.code()));
    ui.label(&h.text);
    if !h.exact {
        ui.weak(t!("lookup.http.class"));
    }
    if let Some(sub) = h.substatus {
        ui.add_space(4.0);
        ui.strong(t!("lookup.http.substatus", substatus = sub));
        match &h.sub_text {
            Some(d) => {
                ui.label(d);
            }
            None => {
                ui.weak(t!("lookup.http.substatus.unknown"));
            }
        }
    }
    ui.add_space(4.0);
    if ui.button(format!("📋 {}", t!("lookup.copy"))).clicked() {
        ui.ctx().copy_text(h.short());
    }
}
