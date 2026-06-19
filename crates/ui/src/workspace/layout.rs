//! Pane resize, window controls, focus, clipboard copy.

use super::*;

impl Workspace {
    /// Snapshot the panes' current widths into `ui` and persist (on resize-end).
    /// The panes own the live widths; this is the persistence buffer.
    pub(crate) fn persist_pane_widths(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let sidebar = f32::from(self.sidebar.read(cx).width());
        let preview = f32::from(self.preview.read(cx).width());
        let jobs = f32::from(self.jobs_width);
        let (date, size) = {
            let e = self.explorer.read(cx);
            (f32::from(e.col_date_width()), f32::from(e.col_size_width()))
        };
        let unchanged = (
            self.ui.sidebar_width,
            self.ui.preview_width,
            self.ui.jobs_width,
            self.ui.col_date_width,
            self.ui.col_size_width,
        ) == (Some(sidebar), Some(preview), Some(jobs), Some(date), Some(size));
        if !unchanged {
            self.ui.sidebar_width = Some(sidebar);
            self.ui.preview_width = Some(preview);
            self.ui.jobs_width = Some(jobs);
            self.ui.col_date_width = Some(date);
            self.ui.col_size_width = Some(size);
            self.save_ui();
        }
    }

    pub(crate) fn save_ui(&self) {
        self.app.db.save_ui(&self.ui);
    }

    pub(crate) fn dock_is(&self, panel: DockPanel) -> bool {
        self.dock == Some(panel)
    }

    /// Set the active right-dock panel (exclusive). Only the preview choice is
    /// persisted; tasks is transient.
    pub(crate) fn set_dock(&mut self, dock: Option<DockPanel>, cx: &mut Context<Self>) {
        self.dock = dock;
        let preview_open = dock == Some(DockPanel::Preview);
        if self.ui.preview_open != preview_open {
            self.ui.preview_open = preview_open;
            self.save_ui();
        }
        if preview_open {
            self.preview.update(cx, |p, cx| p.refresh(cx));
        }
        cx.notify();
    }

    /// Toggle a dock panel: activate it, or close the dock if it's already active.
    pub(crate) fn toggle_dock(&mut self, panel: DockPanel, cx: &mut Context<Self>) {
        let next = (self.dock != Some(panel)).then_some(panel);
        self.set_dock(next, cx);
    }

    pub(crate) fn set_jobs_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        if self.jobs_width != width {
            self.jobs_width = width;
            cx.notify();
        }
    }

    pub(crate) fn reset_jobs_width(&mut self, cx: &mut Context<Self>) {
        self.set_jobs_width(px(JOBS_W), cx);
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
        } else if self.open_remote.is_some() {
            self.enter_explorer(window, cx);
        }
    }

    pub(crate) fn focus_sidebar(&mut self, _: &FocusSidebar, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_sidebar_pane(window, cx);
    }

    pub(crate) fn focus_explorer(&mut self, _: &FocusExplorer, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_remote.is_some() {
            self.enter_explorer(window, cx);
        }
    }

    pub(crate) fn copy_text(&self) -> String {
        match &self.open_remote {
            Some(r) => format!("{r}:{}", self.path),
            None => String::new(),
        }
    }

    pub(crate) fn copy_button(
        &self,
        id: impl Into<gpui::ElementId>,
        source: CopySource,
        text: String,
        tip: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let done = self.copied == Some(source);
        h_flex()
            .id(id)
            .size(rem(22.0))
            .flex_shrink_0()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .hover(|s| s.bg(rgba(OVERLAY)))
            .tooltip(tooltip_text(if done { "Copied" } else { tip }))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.copy_with_feedback(source, text.clone(), cx)
            }))
            .child(
                svg()
                    .path(if done { "icons/check.svg" } else { "icons/copy.svg" })
                    .size(rem(13.0))
                    .text_color(rgb(if done { SUCCESS } else { FG_MUTED })),
            )
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
