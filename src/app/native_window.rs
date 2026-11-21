//! Tool windows (#68): a window with its own OS window (egui immediate viewport) that can be
//! moved outside the main window and onto another monitor, or – when the setting
//! `Config::separate_windows` is off or the backend has no native windows
//! (`Context::embed_viewports`) – an `egui::Window` inside the main window.
//!
//! Adapted from MM PortLens `native_window.rs`, without its own zoom: the text size follows the
//! user interface zoom of the main window (the zoom factor belongs to the context all viewports
//! share). The position and size are kept in a `config::ToolWindowState`.

use crate::app::*;
use crate::config::ToolWindowState;
use std::sync::OnceLock;

/// The fixed look of a tool window.
pub(in crate::app) struct ToolSpec {
    pub title: String,
    /// Inner size of the first window (logical pixels) and its minimum.
    pub default_size: [f32; 2],
    pub min_size: [f32; 2],
    /// The content scrolls when the window is smaller than it (false: the content has its own
    /// scroll areas and uses the whole height).
    pub scroll: bool,
}

impl ToolSpec {
    /// A window whose content scrolls.
    pub(in crate::app) fn new(title: String, default_size: [f32; 2], min_size: [f32; 2]) -> Self {
        Self {
            title,
            default_size,
            min_size,
            scroll: true,
        }
    }

    /// The content manages its own scroll areas.
    pub(in crate::app) fn no_scroll(mut self) -> Self {
        self.scroll = false;
        self
    }
}

/// One tool window: open or not, and the geometry its native window opened with.
pub(in crate::app) struct ToolWindow {
    /// Unique key: the viewport id and the egui id are derived from it.
    key: &'static str,
    open: bool,
    /// Bring the window to the front in the next frame.
    focus: bool,
    /// The window cannot be closed (× / Esc are ignored), e.g. while an update is installed.
    pub(in crate::app) locked: bool,
    /// Position and size the native window was opened with. Fixed at its first frame: a builder
    /// that changes between frames moves the open window.
    opened_at: Option<(Option<[f32; 2]>, [f32; 2])>,
}

/// The window icon of the tool windows (the program icon), drawn once.
fn icon_data() -> Arc<egui::IconData> {
    static ICON: OnceLock<Arc<egui::IconData>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(egui::IconData {
            rgba: crate::icon::rgba(64),
            width: 64,
            height: 64,
        })
    })
    .clone()
}

impl ToolWindow {
    pub(in crate::app) const fn new(key: &'static str) -> Self {
        Self {
            key,
            open: false,
            focus: false,
            locked: false,
            opened_at: None,
        }
    }

    pub(in crate::app) fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the window, or brings the open one to the front.
    pub(in crate::app) fn open(&mut self) {
        self.open = true;
        self.focus = true;
    }

    pub(in crate::app) fn close(&mut self) {
        self.open = false;
        self.focus = false;
        self.opened_at = None;
    }

    /// Draws the window when it is open. `state` gets the position and size of a native window;
    /// × and Esc (native window, when no field or popup is active) close it.
    /// Returns `true` when `state` changed.
    pub(in crate::app) fn show(
        &mut self,
        ctx: &egui::Context,
        native: bool,
        spec: &ToolSpec,
        state: &mut ToolWindowState,
        content: impl FnOnce(&mut egui::Ui),
    ) -> bool {
        if !self.open {
            return false;
        }
        let before = state.clone();
        let close = if native {
            self.show_native(ctx, spec, state, content)
        } else {
            self.opened_at = None;
            self.show_embedded(ctx, spec, content)
        };
        if close && !self.locked {
            self.close();
        }
        *state != before
    }

    fn show_native(
        &mut self,
        ctx: &egui::Context,
        spec: &ToolSpec,
        state: &mut ToolWindowState,
        content: impl FnOnce(&mut egui::Ui),
    ) -> bool {
        let (pos, size) = *self.opened_at.get_or_insert_with(|| {
            let size = state
                .size
                .map(|[w, h]| [w.max(spec.min_size[0]), h.max(spec.min_size[1])])
                .unwrap_or(spec.default_size);
            (state.pos, size)
        });
        let mut builder = egui::ViewportBuilder::default()
            .with_title(&spec.title)
            .with_icon(icon_data())
            .with_inner_size(size)
            .with_min_inner_size(spec.min_size);
        if let Some(pos) = pos {
            builder = builder.with_position(pos);
        }
        let viewport = egui::ViewportId::from_hash_of(self.key);
        if self.focus {
            self.focus = false;
            ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Focus);
        }
        let mut content = Some(content);
        let scroll = spec.scroll;
        let locked = self.locked;
        ctx.show_viewport_immediate(viewport, builder, |ui, _class| {
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                if let Some(c) = content.take() {
                    scrolled(ui, scroll, c);
                }
            });
            // Not inside `input`: the context is locked there.
            let ui_zoom = ctx.zoom_factor();
            let (close_requested, esc, geometry) = ctx.input(|i| {
                let vp = i.viewport();
                (
                    vp.close_requested(),
                    i.key_pressed(egui::Key::Escape),
                    window_geometry(vp, ui_zoom),
                )
            });
            if let Some(g) = geometry {
                state.pos = g.pos;
                state.size = g.size;
            }
            // Esc belongs to an active field or an open popup (color picker, combo box).
            let esc =
                esc && ctx.memory(|m| m.focused().is_none()) && !egui::Popup::is_any_open(&ctx);
            if close_requested && locked {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
            close_requested || esc
        })
    }

    fn show_embedded(
        &mut self,
        ctx: &egui::Context,
        spec: &ToolSpec,
        content: impl FnOnce(&mut egui::Ui),
    ) -> bool {
        let id = egui::Id::unique(self.key);
        if self.focus {
            self.focus = false;
            ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
        }
        let mut open = true;
        egui::Window::new(&spec.title)
            .id(id)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(spec.default_size)
            .min_size(spec.min_size)
            .show(ctx, |ui| scrolled(ui, spec.scroll, content));
        !open
    }
}

/// Runs `content`, in a scroll area when `scroll`.
fn scrolled(ui: &mut egui::Ui, scroll: bool, content: impl FnOnce(&mut egui::Ui)) {
    if scroll {
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, content);
    } else {
        content(ui);
    }
}

impl App {
    /// Shows the tool window `win` (a field of `App`) when it is open. `content` gets the app,
    /// the `Ui` and a flag it clears to close the window (the × button and Esc close it as well).
    /// The window is taken out of `App` while it is drawn, because `content` borrows the app.
    /// Returns whether the window is still open. Position and size are kept in `Config::windows`.
    pub(in crate::app) fn show_tool(
        &mut self,
        ctx: &egui::Context,
        win: impl Fn(&mut App) -> &mut ToolWindow,
        spec: ToolSpec,
        content: impl FnOnce(&mut App, &mut egui::Ui, &mut bool),
    ) -> bool {
        if !win(self).is_open() {
            return false;
        }
        let key = win(self).key;
        let native = self.cfg.separate_windows && !ctx.embed_viewports();
        let mut w = std::mem::replace(win(self), ToolWindow::new(key));
        let mut state = self.cfg.windows.get(key).cloned().unwrap_or_default();
        let mut keep = true;
        let moved = w.show(ctx, native, &spec, &mut state, |ui| {
            content(self, ui, &mut keep)
        });
        if !keep {
            w.close();
        }
        let open = w.is_open();
        *win(self) = w;
        if moved {
            self.cfg.windows.insert(key.to_owned(), state);
            self.cfg_dirty = true;
        }
        open
    }
}

/// The position and size of a native window in logical pixels (egui points include the zoom);
/// `None` while minimized or before the size is known. A maximized window keeps the last normal
/// geometry (not stored).
pub(in crate::app) fn window_geometry(vp: &egui::ViewportInfo, zoom: f32) -> Option<WindowState> {
    if vp.minimized == Some(true) || vp.maximized == Some(true) {
        return None;
    }
    let inner = vp.inner_rect?;
    Some(WindowState {
        pos: vp.outer_rect.map(|r| [r.min.x * zoom, r.min.y * zoom]),
        size: Some([inner.width() * zoom, inner.height() * zoom]),
        maximized: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_in_logical_pixels() {
        let vp = egui::ViewportInfo {
            outer_rect: Some(egui::Rect::from_min_size(
                egui::pos2(100.0, 50.0),
                egui::vec2(410.0, 330.0),
            )),
            inner_rect: Some(egui::Rect::from_min_size(
                egui::pos2(105.0, 80.0),
                egui::vec2(400.0, 300.0),
            )),
            ..Default::default()
        };
        let g = window_geometry(&vp, 1.5).unwrap();
        assert_eq!(g.pos, Some([150.0, 75.0]));
        assert_eq!(g.size, Some([600.0, 450.0]));
        let min = egui::ViewportInfo {
            minimized: Some(true),
            ..vp
        };
        assert_eq!(window_geometry(&min, 1.0), None);
    }

    #[test]
    fn open_focus_close() {
        let mut w = ToolWindow::new("test_tool");
        assert!(!w.is_open());
        w.open();
        assert!(w.is_open() && w.focus);
        w.opened_at = Some((None, [1.0, 1.0]));
        // Opening again only focuses.
        w.focus = false;
        w.open();
        assert!(w.focus && w.opened_at.is_some());
        // Closing forgets the opening geometry, so the next one uses the remembered state.
        w.close();
        assert!(!w.is_open() && w.opened_at.is_none());
    }
}
