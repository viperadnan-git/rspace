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
            // Always collapse into the left group, appending the right group's tabs
            // after it (order preserved); keep the focused tab active.
            let focused_id = self.active().id;
            let mut right = self.groups.remove(1).tabs;
            self.groups[0].tabs.append(&mut right);
            self.active_group = 0;
            if let Some(ix) = self.groups[0].tabs.iter().position(|t| t.id == focused_id) {
                self.groups[0].active = ix;
            }
            self.clear_compare(cx);
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
    /// Swap the two groups left↔right (source becomes destination). Invalidates the
    /// compare, since direction reversed.
    pub(crate) fn swap_panes(&mut self, cx: &mut Context<Self>) {
        if self.groups.len() < 2 {
            return;
        }
        self.groups.swap(0, 1);
        self.active_group = 1 - self.active_group;
        self.clear_compare(cx);
        self.set_active_polling(cx);
        self.retarget_preview(cx);
        cx.notify();
    }

    pub(crate) fn has_compare(&self) -> bool {
        self.compare.is_some()
    }

    pub(crate) fn focus_group(&mut self, g: usize, cx: &mut Context<Self>) {
        if g != self.active_group && g < self.groups.len() {
            self.active_group = g;
            self.retarget_preview(cx);
            cx.notify();
        }
    }

    pub(crate) fn action_toggle_sync(&mut self, _: &ToggleSync, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_sync_popover(cx);
    }

    /// Open (or close) the sync popover anchored to the status bar.
    pub(crate) fn toggle_sync_popover(&mut self, cx: &mut Context<Self>) {
        let open = self.menus.sync_popover_open;
        self.close_menus();
        self.menus.sync_popover_open = !open;
        cx.notify();
    }

    pub(crate) fn is_split(&self) -> bool {
        self.groups.len() > 1
    }

    pub(crate) fn comparing(&self) -> bool {
        self.comparing
    }

    /// `(left, right)` endpoint labels (`remote:path`) when split, for the Sync panel.
    pub(crate) fn sync_endpoints(&self) -> Option<(String, String)> {
        if self.groups.len() < 2 {
            return None;
        }
        let label = |p: &Pane| match &p.open_remote {
            Some(r) if p.path.is_empty() => format!("{r}:"),
            Some(r) => format!("{r}:{}", p.path),
            None => "—".to_string(),
        };
        Some((label(&self.groups[0].active_tab().pane), label(&self.groups[1].active_tab().pane)))
    }

    /// `(differ, src_only, dst_only, matched)` from the last compare.
    pub(crate) fn compare_counts(&self) -> Option<(usize, usize, usize, usize)> {
        self.compare.as_ref().map(|entries| {
            let (mut differ, mut src, mut dst, mut matched) = (0, 0, 0, 0);
            for e in entries {
                match e.state {
                    DiffState::Differ => differ += 1,
                    DiffState::SrcOnly => src += 1,
                    DiffState::DstOnly => dst += 1,
                    DiffState::Match => matched += 1,
                    DiffState::Error => {}
                }
            }
            (differ, src, dst, matched)
        })
    }

    /// Compare the two split panes (left = source, right = destination) via rclone's
    /// own check, then overlay the result onto both file lists.
    pub(crate) fn run_compare(&mut self, cx: &mut Context<Self>) {
        if self.groups.len() < 2 {
            return;
        }
        let (lr, lp, rr, rp) = {
            let left = &self.groups[0].active_tab().pane;
            let right = &self.groups[1].active_tab().pane;
            match (left.open_remote.clone(), right.open_remote.clone()) {
                (Some(lr), Some(rr)) => (lr, left.path.clone(), rr, right.path.clone()),
                _ => {
                    self.toast_sticky("Open a folder in both panes to compare".to_string(), true, cx);
                    return;
                }
            }
        };
        let left_ex = self.groups[0].active_tab().pane.explorer.clone();
        let right_ex = self.groups[1].active_tab().pane.explorer.clone();
        let (src_fs, dst_fs) = (format!("{lr}:{lp}"), format!("{rr}:{rp}"));
        let service = self.app.service.clone();
        self.comparing = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = service.compare(src_fs, dst_fs).await;
            this.update(cx, |this, cx| {
                this.comparing = false;
                match res {
                    Ok(entries) => {
                        left_ex.update(cx, |e, cx| e.set_diff(&entries, &lp, cx));
                        right_ex.update(cx, |e, cx| e.set_diff(&entries, &rp, cx));
                        this.compare = Some(entries);
                    }
                    Err(e) => this.toast_sticky(format!("Compare failed: {e}"), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn sync_mode(&self) -> SyncMode {
        self.sync_mode
    }

    pub(crate) fn bisync_resync(&self) -> bool {
        self.bisync_resync
    }

    pub(crate) fn set_sync_mode(&mut self, mode: SyncMode, cx: &mut Context<Self>) {
        self.sync_mode = mode;
        cx.notify();
    }

    pub(crate) fn toggle_resync(&mut self, cx: &mut Context<Self>) {
        self.bisync_resync = !self.bisync_resync;
        cx.notify();
    }

    /// Run the chosen sync between the two panes (left = source, right = dest).
    /// Destructive modes confirm first; the compare result is the preview.
    pub(crate) fn start_sync(&mut self, cx: &mut Context<Self>) {
        if self.groups.len() < 2 {
            return;
        }
        let (mode, resync) = (self.sync_mode, self.bisync_resync);
        let (lr, lp, rr, rp) = {
            let left = &self.groups[0].active_tab().pane;
            let right = &self.groups[1].active_tab().pane;
            match (left.open_remote.clone(), right.open_remote.clone()) {
                (Some(lr), Some(rr)) => (lr, left.path.clone(), rr, right.path.clone()),
                _ => {
                    self.toast_sticky("Open a folder in both panes to sync".to_string(), true, cx);
                    return;
                }
            }
        };
        if mode.destructive() {
            let message = match mode {
                SyncMode::Mirror => {
                    "Make the right folder match the left, deleting anything extra on the right. This can't be undone."
                }
                SyncMode::Bisync if resync => {
                    "Reconcile both folders and establish a new baseline (resync). Existing differences are resolved by preferring the left."
                }
                SyncMode::Bisync => "Reconcile both folders two-way, applying each side's changes to the other.",
                SyncMode::Copy => "",
            };
            self.ask_confirm(
                format!("{}?", mode.label()),
                message.to_string(),
                mode.label(),
                true,
                move |this, cx| this.spawn_sync(mode, lr, lp, rr, rp, resync, cx),
                cx,
            );
        } else {
            self.spawn_sync(mode, lr, lp, rr, rp, resync, cx);
        }
    }

    /// Drop the compare result and its row markers (on collapse or a new pairing).
    pub(crate) fn clear_compare(&mut self, cx: &mut Context<Self>) {
        self.compare = None;
        for group in &self.groups {
            group.active_tab().pane.explorer.update(cx, |e, cx| e.clear_diff(cx));
        }
        cx.notify();
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
