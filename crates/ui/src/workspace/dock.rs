//! The right dock: a single panel at a time (preview xor tasks), a shared
//! resizable width, and uniform chrome (header + close + resize). Each panel's
//! state lives in its own entity; the dock only owns layout and which panel
//! shows. Adding a panel: add a [`Panel`] variant, a header-actions arm, and a
//! body arm here — nothing else changes.

use super::*;

impl Workspace {
    pub(crate) fn dock_is(&self, panel: Panel) -> bool {
        self.dock == Some(panel)
    }

    /// Toggle a panel: show it, or close the dock if it already shows.
    pub(crate) fn toggle_panel(&mut self, panel: Panel, cx: &mut Context<Self>) {
        let next = (self.dock != Some(panel)).then_some(panel);
        self.show_panel(next, cx);
    }

    /// Show `panel` (or close the dock with `None`). Binds the preview to the
    /// active tab when it appears. The dock is session state — never restored on
    /// launch — so it always starts closed.
    pub(crate) fn show_panel(&mut self, panel: Option<Panel>, cx: &mut Context<Self>) {
        self.dock = panel;
        if panel == Some(Panel::Preview) {
            let explorer = self.active().explorer.clone();
            self.preview.update(cx, |p, cx| p.set_explorer(explorer, cx));
        }
        cx.notify();
    }

    pub(crate) fn close_dock(&mut self, cx: &mut Context<Self>) {
        if self.dock.is_some() {
            self.show_panel(None, cx);
        }
    }

    /// Re-point the preview at the active tab's explorer (on tab switch); no-op
    /// unless the preview panel is showing.
    pub(crate) fn retarget_preview(&mut self, cx: &mut Context<Self>) {
        if self.dock_is(Panel::Preview) {
            let explorer = self.active().explorer.clone();
            self.preview.update(cx, |p, cx| p.set_explorer(explorer, cx));
        }
    }

    pub(crate) fn set_dock_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        if self.dock_width != width {
            self.dock_width = width;
            cx.notify();
        }
    }

    pub(crate) fn reset_dock_width(&mut self, cx: &mut Context<Self>) {
        self.set_dock_width(px(PREVIEW_W), cx);
    }

    /// The dock, if a panel is open: chrome (width, resize, header) + the panel's
    /// body. Returns `None` (skips the slot) when the dock is closed.
    pub(crate) fn render_dock(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let panel = self.dock?;
        Some(
            v_flex()
                .relative()
                .w(self.dock_width)
                .min_h(px(0.0))
                .flex_shrink_0()
                .overflow_hidden()
                .bg(rgb(INSET))
                .border_l_1()
                .border_color(rgb(BORDER_MUTED))
                .child(self.resize_handle("dock-resize", ResizeTarget::Dock, cx))
                .child(self.render_dock_header(panel, cx))
                // A flex-column body so the panel's `flex_1` content (the task list,
                // the preview) gets vertical space — a plain `div` is flex-row.
                .child(v_flex().flex_1().min_h(px(0.0)).overflow_hidden().child(self.render_dock_body(panel, cx))),
        )
    }

    fn render_dock_header(&self, panel: Panel, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .h(px(PANE_HEADER_H))
            .justify_between()
            .items_center()
            .px_3()
            .border_b_1()
            .border_color(rgb(BORDER_MUTED))
            .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child(panel.title()))
            .child(
                h_flex()
                    .gap_1()
                    .children(self.dock_header_actions(panel, cx))
                    .child(
                        icon_button("dock-close", "icons/x.svg")
                            .tooltip(tooltip_text("Close"))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_dock(cx))),
                    ),
            )
    }

    /// Panel-specific header buttons (left of the close button).
    fn dock_header_actions(&self, panel: Panel, cx: &mut Context<Self>) -> Vec<AnyElement> {
        match panel {
            Panel::Tasks if self.jobs.read(cx).has_finished() => vec![
                icon_button("clear-finished", "icons/trash.svg")
                    .tooltip(tooltip_text("Clear finished"))
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.request_clear_finished(cx)),
                    )
                    .into_any_element(),
            ],
            _ => Vec::new(),
        }
    }

    fn render_dock_body(&self, panel: Panel, cx: &mut Context<Self>) -> AnyElement {
        match panel {
            Panel::Preview => self.preview().into_any_element(),
            Panel::Tasks => self.render_tasks_body(cx),
        }
    }
}
