//! Pane resize, window controls, focus, clipboard copy.

use super::*;

impl Workspace {
    pub(crate) fn on_column_drag(&mut self, e: &DragMoveEvent<DragColumn>, _: &mut Window, cx: &mut Context<Self>) {
        let x = f32::from(e.event.position.x);
        let right = f32::from(e.bounds.right()) - TABLE_PAD;
        // Column order is Name (flex), Size, Date — Date is flush right; Size is
        // flush to its left. Anchor each from the content edge so the dragged
        // divider tracks the cursor exactly.
        let date_w = f32::from(self.col_date_width);
        let (raw, current) = match e.drag(cx).0 {
            Column::Date => (right - x, &mut self.col_date_width),
            Column::Size => (right - date_w - x, &mut self.col_size_width),
        };
        let width = px(raw.clamp(COL_MIN, COL_MAX));
        if width != *current {
            *current = width;
            cx.notify();
        }
    }

    pub(crate) fn reset_column(&mut self, column: Column, cx: &mut Context<Self>) {
        match column {
            Column::Date => self.col_date_width = px(COL_DATE),
            Column::Size => self.col_size_width = px(COL_SIZE),
        }
        cx.notify();
    }

    pub(crate) fn persist_pane_widths(&mut self, _: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        let (sidebar, preview, date, size) = (
            f32::from(self.sidebar_width),
            f32::from(self.preview_width),
            f32::from(self.col_date_width),
            f32::from(self.col_size_width),
        );
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
        self.db.save_ui(&self.ui);
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

    pub(crate) fn toggle_pane(&mut self, _: &TogglePane, _window: &mut Window, cx: &mut Context<Self>) {
        self.pane = match self.pane {
            Pane::Sidebar if self.open_remote.is_some() => Pane::Explorer,
            Pane::Sidebar => Pane::Sidebar,
            Pane::Explorer => Pane::Sidebar,
        };
        cx.notify();
    }

    pub(crate) fn focus_sidebar(&mut self, _: &FocusSidebar, _window: &mut Window, cx: &mut Context<Self>) {
        self.pane = Pane::Sidebar;
        cx.notify();
    }

    pub(crate) fn focus_explorer(&mut self, _: &FocusExplorer, _window: &mut Window, cx: &mut Context<Self>) {
        if self.open_remote.is_some() {
            self.pane = Pane::Explorer;
            cx.notify();
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
