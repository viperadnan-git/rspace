//! Splitting the workspace into two side-by-side [`PaneGroup`]s (source left,
//! dest right) — each a full browser with its own tabs. The surface sync compares
//! and reconciles. Capped at two groups; the divider sets the split ratio, and
//! clicking a group makes it the focused one.

use super::*;

impl Workspace {
    /// Split into two groups (cloning the focused tab's location into the new
    /// right group), or merge back to one (the other group's tabs append to the
    /// survivor — nothing is lost).
    pub(crate) fn toggle_split(&mut self, _: &ToggleSplit, window: &mut Window, cx: &mut Context<Self>) {
        if self.groups.len() > 1 {
            let other = 1 - self.active_group;
            let mut moved = self.groups.remove(other).tabs;
            // Only one group remains; the survivor settles at index 0.
            self.active_group = 0;
            self.groups[0].tabs.append(&mut moved);
        } else {
            let new_tab = self.clone_focused_tab(window, cx);
            self.groups.push(PaneGroup::new(new_tab));
            self.active_group = self.groups.len() - 1;
        }
        self.set_active_polling(cx);
        self.retarget_preview(cx);
        self.focus_active_tab(window, cx);
        cx.notify();
    }

    /// A fresh tab showing the same location as the focused one.
    fn clone_focused_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Tab {
        let weak = cx.entity().downgrade();
        let (sort, refresh_secs) = {
            let s = self.store.get();
            ((s.sort_field, s.sort_order), s.refresh_secs)
        };
        let (cols, remote, path) = {
            let p = self.focused_pane();
            let e = p.explorer.read(cx);
            ((e.col_date_width(), e.col_size_width()), p.open_remote.clone(), p.path.clone())
        };
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let mut tab = Self::build_tab(id, &weak, &self.app.service, sort, refresh_secs, cols, window, cx);
        if let Some(remote) = remote {
            tab.pane.open_remote = Some(remote.clone());
            tab.pane.path = path.clone();
            tab.pane.history = vec![Location { remote: remote.clone(), path: path.clone(), selected: None }];
            tab.pane.explorer.update(cx, |e, cx| e.show(Some(remote), path, None, cx));
        }
        tab
    }

    /// Make group `g` the focused one (keyboard + preview target). Only sets the
    /// flag — gpui focus follows the clicked element on its own.
    pub(crate) fn focus_group(&mut self, g: usize, cx: &mut Context<Self>) {
        if g != self.active_group && g < self.groups.len() {
            self.active_group = g;
            self.retarget_preview(cx);
            cx.notify();
        }
    }

    pub(crate) fn set_split_ratio(&mut self, ratio: f32, cx: &mut Context<Self>) {
        if (self.split_ratio - ratio).abs() > f32::EPSILON {
            self.split_ratio = ratio;
            cx.notify();
        }
    }

    /// Translate a divider drag (window x) into a split ratio, accounting for the
    /// sidebar and any open dock so the divider tracks the cursor.
    pub(crate) fn resize_split(&mut self, x: f32, window: &Window, cx: &mut Context<Self>) {
        let viewport = f32::from(window.viewport_size().width);
        let sidebar = f32::from(self.sidebar.read(cx).width());
        let dock = if self.dock.is_some() { f32::from(self.dock_width) } else { 0.0 };
        let avail = (viewport - sidebar - dock).max(1.0);
        let ratio = ((x - sidebar) / avail).clamp(SPLIT_MIN, SPLIT_MAX);
        self.set_split_ratio(ratio, cx);
    }

    /// One group's column: its tab strip above its active tab's body, made focusable
    /// by a click anywhere within it.
    pub(crate) fn render_group_column(&self, g: usize, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .capture_any_mouse_down(cx.listener(move |this, _: &MouseDownEvent, _, cx| this.focus_group(g, cx)))
            .child(self.render_tab_strip(g, cx))
            .child(self.render_pane_column(&self.groups[g].active_tab().pane, cx))
            .into_any_element()
    }

    /// One pane's body: the welcome screen when it has no remote open, else its
    /// action bar above its explorer.
    pub(crate) fn render_pane_column(&self, pane: &Pane, cx: &mut Context<Self>) -> AnyElement {
        if pane.open_remote.is_none() {
            return self.render_welcome(cx).into_any_element();
        }
        v_flex()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .overflow_hidden()
            .bg(rgb(INSET))
            .child(pane.action_bar.clone())
            .child(pane.explorer.clone())
            .into_any_element()
    }

    /// A 1px line with a wider invisible grab zone overlapping both panes (Zed-style).
    pub(crate) fn pane_divider(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div().relative().w(px(1.0)).h_full().flex_shrink_0().bg(rgb(BORDER_MUTED)).child(
            div()
                .id("pane-split-resize")
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(-3.0))
                .w(px(7.0))
                .cursor_col_resize()
                .occlude()
                .on_drag(DragResize(ResizeTarget::PaneSplit), move |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| DragResize(ResizeTarget::PaneSplit))
                })
                .on_click(cx.listener(|this, e: &ClickEvent, _, cx| {
                    if e.click_count() >= 2 {
                        this.set_split_ratio(0.5, cx);
                    }
                })),
        )
    }
}
