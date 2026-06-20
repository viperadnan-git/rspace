//! Pane resize, window controls, focus, clipboard copy.

use super::*;

impl Workspace {
    /// Snapshot the panes' current widths into `ui` and persist (on resize-end).
    /// The panes own the live widths; this is the persistence buffer. The dock
    /// width is stored as `preview_width` (one width shared by all dock panels).
    pub(crate) fn persist_pane_widths(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let sidebar = f32::from(self.sidebar.read(cx).width());
        let dock = f32::from(self.dock_width);
        let (date, size) = {
            let e = self.explorer();
            let e = e.read(cx);
            (f32::from(e.col_date_width()), f32::from(e.col_size_width()))
        };
        let unchanged = (
            self.ui.sidebar_width,
            self.ui.preview_width,
            self.ui.col_date_width,
            self.ui.col_size_width,
        ) == (Some(sidebar), Some(dock), Some(date), Some(size));
        if !unchanged {
            self.ui.sidebar_width = Some(sidebar);
            self.ui.preview_width = Some(dock);
            self.ui.col_date_width = Some(date);
            self.ui.col_size_width = Some(size);
            self.save_ui();
        }
    }

    pub(crate) fn save_ui(&self) {
        self.app.db.save_ui(&self.ui);
    }

    pub(crate) fn minimize(&mut self, _: &Minimize, window: &mut Window, _cx: &mut Context<Self>) {
        window.minimize_window();
    }

    pub(crate) fn zoom(&mut self, _: &Zoom, window: &mut Window, _cx: &mut Context<Self>) {
        window.zoom_window();
    }

    pub(crate) fn toggle_fullscreen(&mut self, _: &ToggleFullscreen, window: &mut Window, _cx: &mut Context<Self>) {
        window.toggle_fullscreen();
    }

    /// The base UI font size in px (drives the window rem size), clamped.
    pub(crate) fn ui_font_size(&self) -> f32 {
        self.store.get().ui_font_size.clamp(UI_FONT_MIN, UI_FONT_MAX)
    }

    pub(crate) fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_font_size(1.0, cx);
    }

    pub(crate) fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_font_size(-1.0, cx);
    }

    pub(crate) fn zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        self.store.update(|s| s.ui_font_size = UI_FONT_DEFAULT);
        cx.notify();
    }

    fn adjust_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        let next = (self.ui_font_size() + delta).round().clamp(UI_FONT_MIN, UI_FONT_MAX);
        self.store.update(|s| s.ui_font_size = next);
        cx.notify();
    }

    pub(crate) fn copy_to_clipboard(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    pub(crate) fn toggle_pane(&mut self, _: &TogglePane, window: &mut Window, cx: &mut Context<Self>) {
        if self.explorer_focused(window, cx) {
            self.focus_sidebar_pane(window, cx);
        } else if self.active().open_remote.is_some() {
            self.enter_explorer(window, cx);
        }
    }

    pub(crate) fn focus_sidebar(&mut self, _: &FocusSidebar, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_sidebar_pane(window, cx);
    }

    pub(crate) fn focus_explorer(&mut self, _: &FocusExplorer, window: &mut Window, cx: &mut Context<Self>) {
        if self.active().open_remote.is_some() {
            self.enter_explorer(window, cx);
        }
    }

    pub(crate) fn copy_text(&self) -> String {
        let tab = self.active();
        match &tab.open_remote {
            Some(r) => format!("{r}:{}", tab.path),
            None => String::new(),
        }
    }

    pub(crate) fn copy_with_feedback(&mut self, source: CopySource, text: String, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = Some(source);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1200)).await;
            this.update(cx, |this, cx| {
                if this.copied == Some(source) {
                    this.copied = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

}
