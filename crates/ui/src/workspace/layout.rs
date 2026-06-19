//! Pane resize, window controls, focus, clipboard copy.

use super::*;

impl Workspace {
    /// Snapshot the panes' current widths into `ui` and persist (on resize-end).
    /// The panes own the live widths; this is the persistence buffer.
    pub(crate) fn persist_pane_widths(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let sidebar = f32::from(self.sidebar.read(cx).width());
        let preview = f32::from(self.preview.read(cx).width());
        let (date, size) = {
            let e = self.explorer.read(cx);
            (f32::from(e.col_date_width()), f32::from(e.col_size_width()))
        };
        let unchanged = (self.ui.sidebar_width, self.ui.preview_width, self.ui.col_date_width, self.ui.col_size_width)
            == (Some(sidebar), Some(preview), Some(date), Some(size));
        if !unchanged {
            self.ui.sidebar_width = Some(sidebar);
            self.ui.preview_width = Some(preview);
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
            .size(px(22.0))
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
                    .size(px(13.0))
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
