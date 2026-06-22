//! The right dock: a single panel at a time (preview xor tasks), a shared
//! resizable width, and uniform chrome (header + close + resize). Each panel's
//! state lives in its own entity; the dock only owns layout and which panel
//! shows. Adding a panel: add a [`Panel`] variant, its backing entity, and one arm
//! in [`Workspace::dock_view`] (title + header actions + body) — nothing else.

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
            let explorer = self.explorer();
            self.preview.update(cx, |p, cx| p.set_explorer(explorer, cx));
        }
        cx.notify();
    }

    pub(crate) fn close_dock(&mut self, cx: &mut Context<Self>) {
        if self.dock.is_some() {
            self.show_panel(None, cx);
        }
    }

    /// Re-point the preview at the focused tab's explorer; no-op unless the preview
    /// panel is showing. gpui observation is per-entity, so the subscription must be
    /// re-pointed when the focused explorer changes — called only from
    /// `focused_group_changed`, never ad hoc, so it can't be forgotten.
    pub(crate) fn retarget_preview(&mut self, cx: &mut Context<Self>) {
        if self.dock_is(Panel::Preview) {
            let explorer = self.explorer();
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
        let view = self.dock_view(self.dock?, cx);
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
                .child(self.render_dock_header(view.title, view.header_actions, cx))
                // A flex-column body so the panel's `flex_1` content (the task list,
                // the preview) gets vertical space — a plain `div` is flex-row.
                .child(v_flex().flex_1().min_h(px(0.0)).overflow_hidden().child(view.body)),
        )
    }

    /// Everything a panel contributes to the dock chrome, built in one place: adding
    /// a panel is a single arm here, plus its `Panel` variant and backing entity.
    fn dock_view(&self, panel: Panel, cx: &mut Context<Self>) -> DockView {
        match panel {
            Panel::Preview => DockView {
                title: "PREVIEW",
                header_actions: Vec::new(),
                body: self.preview().into_any_element(),
            },
            Panel::Tasks => DockView {
                title: "TASKS",
                header_actions: self.tasks_header_actions(cx),
                body: self.tasks.clone().into_any_element(),
            },
        }
    }

    /// The Tasks panel's "clear finished" button — workspace logic (jobs queue), so
    /// it lives here rather than in the panel.
    fn tasks_header_actions(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !self.jobs.read(cx).has_finished() {
            return Vec::new();
        }
        vec![icon_button("clear-finished", "icons/trash.svg")
            .tooltip(tooltip_text("Clear finished"))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.request_clear_finished(cx)))
            .into_any_element()]
    }

    fn render_dock_header(
        &self,
        title: &'static str,
        actions: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .h(px(PANE_HEADER_H))
            .justify_between()
            .items_center()
            .px_3()
            .border_b_1()
            .border_color(rgb(BORDER_MUTED))
            .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child(title))
            .child(
                h_flex().gap_1().children(actions).child(
                    icon_button("dock-close", "icons/x.svg")
                        .tooltip(tooltip_text("Close"))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_dock(cx))),
                ),
            )
    }
}

/// One panel's contribution to the dock chrome (see [`Workspace::dock_view`]).
struct DockView {
    title: &'static str,
    header_actions: Vec<AnyElement>,
    body: AnyElement,
}
