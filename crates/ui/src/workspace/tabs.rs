//! Tab lifecycle within [`PaneGroup`]s: open, close, switch, pin, reorder. Tab
//! ids are unique across groups, so menu/drag actions are id-based and locate the
//! owning group; index-based switching (next/prev/jump, ⌘N) acts on the focused
//! group.

use super::*;

impl Workspace {
    /// A tab's strip label: the open folder, the remote at its root, or "New Tab".
    pub(crate) fn tab_title(&self, tab: &Tab) -> String {
        let pane = &tab.pane;
        match &pane.open_remote {
            None => "New Tab".to_string(),
            Some(remote) if pane.path.is_empty() => remote.clone(),
            Some(_) => pane.path.rsplit('/').next().unwrap_or_default().to_string(),
        }
    }

    /// `(group, tab)` indices of the tab with `id`.
    fn locate_tab(&self, id: usize) -> Option<(usize, usize)> {
        self.groups.iter().enumerate().find_map(|(g, group)| {
            group.tabs.iter().position(|t| t.id == id).map(|t| (g, t))
        })
    }

    /// Open a new tab on the welcome screen in the focused group and focus it.
    /// Inherits the focused pane's column widths so it matches what's on screen.
    pub(crate) fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let (sort, refresh_secs) = {
            let s = self.store.get();
            ((s.sort_field, s.sort_order), s.refresh_secs)
        };
        let cols = {
            let e = self.focused_pane().explorer.read(cx);
            (e.col_date_width(), e.col_size_width())
        };
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = Self::build_tab(id, &weak, &self.app.service, sort, refresh_secs, cols, window, cx);
        let group = self.active_group_mut();
        group.tabs.push(tab);
        let last = group.tabs.len() - 1;
        // The funnel highlights the focused pane's remote — None here, a welcome tab.
        self.set_active_tab(last, window, cx);
    }

    /// Open a new tab in group `g`, making that group active first (the `+` button
    /// of a group's strip).
    pub(crate) fn new_tab_in_group(&mut self, g: usize, window: &mut Window, cx: &mut Context<Self>) {
        if g < self.groups.len() {
            self.active_group = g;
        }
        self.new_tab(&NewTab, window, cx);
    }

    /// Open the remote at sidebar index `ix` in a fresh tab (secondary-click).
    pub(crate) fn open_remote_ix_in_new_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(remote) = self.ordered_remotes().get(ix) {
            let name = remote.name.clone();
            self.open_remote_in_new_tab(name, window, cx);
        }
    }

    /// Open `remote` in a fresh tab (remote context menu).
    pub(crate) fn open_remote_in_new_tab(&mut self, remote: String, window: &mut Window, cx: &mut Context<Self>) {
        self.new_tab(&NewTab, window, cx);
        let path = self.remote_paths.get(&remote).cloned().unwrap_or_default();
        self.navigate(remote, path, None, cx);
        self.enter_explorer(window, cx);
    }

    pub(crate) fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        let (g, ix) = (self.active_group, self.active_group().active());
        self.close_tab_at(g, ix, window, cx);
    }

    /// Close tab `ix` in group `g`. The last tab of a split group collapses that
    /// group; the last tab of the sole group resets to the welcome screen.
    fn close_tab_at(&mut self, g: usize, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(group) = self.groups.get(g) else { return };
        if ix >= group.tabs.len() {
            return;
        }
        if group.tabs.len() == 1 {
            if self.groups.len() > 1 {
                self.close_group(g, window, cx);
            } else {
                self.go_home(window, cx);
            }
            return;
        }
        let group = &mut self.groups[g];
        group.tabs.remove(ix);
        if ix < group.active() {
            group.set_active(group.active() - 1);
        }
        group.clamp_active();
        if g == self.active_group {
            self.active_context_changed(window, cx);
        } else {
            self.set_active_polling(cx);
            cx.notify();
        }
    }

    /// Remove group `g` (its tabs go with it); the other group becomes the sole one.
    fn close_group(&mut self, g: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.groups.len() <= 1 {
            return;
        }
        self.groups.remove(g);
        self.clear_compare(cx);
        self.set_active_pane(0, window, cx);
    }

    pub(crate) fn select_tab_id(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((g, ix)) = self.locate_tab(id) else { return };
        if g == self.active_group && ix == self.groups[g].active() {
            return;
        }
        self.activate_tab(g, ix, window, cx);
    }

    /// Switch to tab `ix` within the focused group (next/prev/jump).
    fn select_in_active_group(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.active_group().tabs.len() && ix != self.active_group().active() {
            self.set_active_tab(ix, window, cx);
        }
    }

    pub(crate) fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        let g = self.active_group();
        let n = g.tabs.len();
        if n > 1 {
            self.select_in_active_group((g.active() + 1) % n, window, cx);
        }
    }

    pub(crate) fn prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        let g = self.active_group();
        let n = g.tabs.len();
        if n > 1 {
            self.select_in_active_group((g.active() + n - 1) % n, window, cx);
        }
    }

    /// Make group `g` the focused one and re-sync. The one way to change which pane
    /// is focused.
    pub(crate) fn set_active_pane(&mut self, g: usize, window: &mut Window, cx: &mut Context<Self>) {
        if g < self.groups.len() {
            self.activate_tab(g, self.groups[g].active(), window, cx);
        }
    }

    /// Activate tab `ix` in the focused group and re-sync.
    pub(crate) fn set_active_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.activate_tab(self.active_group, ix, window, cx);
    }

    /// Single entry point for any focus change: point at `(g, ix)` then funnel the
    /// re-sync, so no path can set one without the other.
    pub(crate) fn activate_tab(&mut self, g: usize, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if g >= self.groups.len() {
            return;
        }
        self.active_group = g;
        self.active_group_mut().set_active(ix);
        self.active_context_changed(window, cx);
    }

    /// Preview + polling re-sync after the focused group changes, without moving
    /// keyboard focus (gpui follows clicks itself). The sole caller of
    /// `retarget_preview`.
    pub(crate) fn focused_group_changed(&mut self, cx: &mut Context<Self>) {
        self.set_active_polling(cx);
        self.retarget_preview(cx);
        cx.notify();
    }

    /// Full re-sync when the focused tab changes: polling, preview, sidebar
    /// highlight, menus, and focus. Every focus-changing path funnels here, so none
    /// can forget a step.
    fn active_context_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focused_group_changed(cx);
        let remote = self.focused_pane().open_remote.clone();
        self.select_remote(remote.as_deref(), cx);
        self.close_menus();
        self.focus_active_tab(window, cx);
        cx.notify();
    }

    /// Each group's active tab polls its folder; background tabs don't.
    pub(crate) fn set_active_polling(&mut self, cx: &mut Context<Self>) {
        for group in &self.groups {
            for (ix, tab) in group.tabs.iter().enumerate() {
                let active = ix == group.active();
                tab.pane.explorer.update(cx, |e, cx| e.set_active(active, cx));
            }
        }
    }

    /// Route focus to the focused tab's natural pane: the explorer when a remote is
    /// open, otherwise the sidebar (welcome screen).
    pub(crate) fn focus_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focused_pane().open_remote.is_some() {
            self.focus_explorer_pane(window, cx);
        } else {
            self.focus_sidebar_pane(window, cx);
        }
    }

    fn active_id(&self) -> usize {
        self.active().id
    }

    pub(crate) fn is_tab_pinned(&self, id: usize) -> bool {
        self.locate_tab(id).is_some_and(|(g, t)| self.groups[g].tabs[t].pinned)
    }

    /// Re-point group `g`'s active tab at `id`, clamping if it's gone.
    fn set_active_in_group(&mut self, g: usize, id: usize) {
        let group = &mut self.groups[g];
        let ix = group
            .tabs
            .iter()
            .position(|t| t.id == id)
            .unwrap_or_else(|| group.active());
        group.set_active(ix);
    }

    /// Pin/unpin a tab, regrouping so pinned tabs stay at the front of their strip
    /// (stable order within each group). The active tab follows the move.
    pub(crate) fn toggle_pin_tab(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some((g, ix)) = self.locate_tab(id) else { return };
        let active_id = self.groups[g].active_tab().id;
        let group = &mut self.groups[g];
        group.tabs[ix].pinned = !group.tabs[ix].pinned;
        let tab = group.tabs.remove(ix);
        // Boundary between pinned and unpinned: end of pins when pinning, start of
        // the unpinned run when unpinning — same index.
        let dest = group.tabs.iter().take_while(|t| t.pinned).count();
        group.tabs.insert(dest, tab);
        self.set_active_in_group(g, active_id);
        cx.notify();
    }

    /// Close a specific tab by id (context menu — closes even a pinned tab).
    pub(crate) fn close_tab_id(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((g, ix)) = self.locate_tab(id) {
            self.close_tab_at(g, ix, window, cx);
        }
    }

    /// Close every tab in `id`'s group but `id` and the pinned ones.
    pub(crate) fn close_other_tabs(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((g, _)) = self.locate_tab(id) else { return };
        let keep: Vec<usize> =
            self.groups[g].tabs.iter().filter(|t| t.id == id || t.pinned).map(|t| t.id).collect();
        self.retain_tabs_in_group(g, &keep, id, window, cx);
    }

    /// Close unpinned tabs to the right of `id` in its group.
    pub(crate) fn close_tabs_to_right(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((g, target)) = self.locate_tab(id) else { return };
        let keep: Vec<usize> = self.groups[g]
            .tabs
            .iter()
            .enumerate()
            .filter(|(ix, t)| *ix <= target || t.pinned)
            .map(|(_, t)| t.id)
            .collect();
        self.retain_tabs_in_group(g, &keep, id, window, cx);
    }

    /// Close all unpinned tabs in `id`'s group (pinned tabs stay).
    pub(crate) fn close_all_tabs(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((g, _)) = self.locate_tab(id) else { return };
        let keep: Vec<usize> = self.groups[g].tabs.iter().filter(|t| t.pinned).map(|t| t.id).collect();
        let prefer = keep.first().copied().unwrap_or(id);
        self.retain_tabs_in_group(g, &keep, prefer, window, cx);
    }

    /// Within group `g`, drop every tab whose id isn't in `keep`, then activate
    /// `prefer`. An empty `keep` leaves one welcome tab (or collapses a split
    /// group). `keep` holds a handful of ids, so linear `contains` beats a set.
    fn retain_tabs_in_group(
        &mut self,
        g: usize,
        keep: &[usize],
        prefer: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if keep.is_empty() {
            if self.groups.len() > 1 {
                self.close_group(g, window, cx);
                return;
            }
            let active_id = self.active_id();
            self.groups[g].tabs.retain(|t| t.id == active_id);
            self.groups[g].set_active(0);
            self.go_home(window, cx);
            self.set_active_polling(cx);
            return;
        }
        if keep.len() == self.groups[g].tabs.len() {
            return;
        }
        self.groups[g].tabs.retain(|t| keep.contains(&t.id));
        self.set_active_in_group(g, prefer);
        if g == self.active_group {
            self.active_context_changed(window, cx);
        } else {
            self.set_active_polling(cx);
            cx.notify();
        }
    }

    /// Jump to tab `n` (1-based) in the focused group; `n == 9` lands on the last.
    pub(crate) fn jump_to_tab(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let last = self.active_group().tabs.len().saturating_sub(1);
        let ix = if n >= 9 { last } else { (n - 1).min(last) };
        self.select_in_active_group(ix, window, cx);
    }

    /// Reorder a dragged tab within its strip. Pinned tabs stay grouped at the
    /// front, and tabs only move within their own group and pinned/unpinned run.
    pub(crate) fn reorder_tab(&mut self, from_id: usize, to_id: usize, cx: &mut Context<Self>) {
        if from_id == to_id {
            return;
        }
        let (Some((gf, from)), Some((gt, to))) = (self.locate_tab(from_id), self.locate_tab(to_id))
        else {
            return;
        };
        if gf != gt {
            return;
        }
        let group = &mut self.groups[gf];
        if group.tabs[from].pinned != group.tabs[to].pinned {
            return;
        }
        let active_id = group.active_tab().id;
        let tab = group.tabs.remove(from);
        let dest = if from < to { to - 1 } else { to };
        group.tabs.insert(dest, tab);
        self.set_active_in_group(gf, active_id);
        cx.notify();
    }

    /// Drop a dragged tab onto tab `target`: reorder within a group, or move it to
    /// the target's group at the target's position.
    pub(crate) fn drop_tab_on(&mut self, dragged: usize, target: usize, window: &mut Window, cx: &mut Context<Self>) {
        if dragged == target {
            return;
        }
        let (Some((dg, _)), Some((tg, ti))) = (self.locate_tab(dragged), self.locate_tab(target))
        else {
            return;
        };
        if dg == tg {
            self.reorder_tab(dragged, target, cx);
        } else {
            self.move_tab_to_group(dragged, tg, Some(ti), window, cx);
        }
    }

    /// Drop a dragged tab onto group `g`'s strip (empty area): move it to the end of
    /// that group. A no-op within the same group.
    pub(crate) fn drop_tab_in_group(&mut self, dragged: usize, g: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.locate_tab(dragged).is_some_and(|(dg, _)| dg != g) {
            self.move_tab_to_group(dragged, g, None, window, cx);
        }
    }

    /// Move tab `id` into group `dest` at `at` (or the end), focusing it there. If
    /// the source group empties, it closes (collapsing the split).
    fn move_tab_to_group(
        &mut self,
        id: usize,
        dest: usize,
        at: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((src, st)) = self.locate_tab(id) else { return };
        if src == dest {
            return;
        }
        let tab = self.groups[src].tabs.remove(st);
        {
            let s = &mut self.groups[src];
            if st < s.active() {
                s.set_active(s.active() - 1);
            }
            s.clamp_active();
        }
        let d = &mut self.groups[dest];
        let idx = at.unwrap_or(d.tabs.len()).min(d.tabs.len());
        d.tabs.insert(idx, tab);
        d.set_active(idx);
        if self.groups[src].tabs.is_empty() {
            self.close_group(src, window, cx);
        } else {
            self.activate_tab(dest, idx, window, cx);
        }
    }

    /// Called while a drag hovers tab `id`: after a short dwell the tab activates,
    /// so the user can drop into a different remote/folder. A generation counter
    /// invalidates the timer if the drag moves to another tab first.
    pub(crate) fn spring_hover(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.active().id == id {
            self.spring.clear();
            return;
        }
        let Some(generation) = self.spring.arm(id) else { return };
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(SPRING_LOAD_MS)).await;
            this.update_in(cx, |this, window, cx| {
                if this.spring.live(generation, &id) {
                    this.select_tab_id(id, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Cancel any pending spring activation (drag left the strip / dropped).
    pub(crate) fn spring_clear(&mut self) {
        self.spring.clear();
    }

    /// Drop dragged items onto a tab header: into that tab's open folder, with the
    /// source-relative move/copy rule (copy across remotes, move within one).
    pub(crate) fn drop_into_tab(&mut self, id: usize, dragged: &DraggedEntry, mods: Modifiers, cx: &mut Context<Self>) {
        let Some((g, ix)) = self.locate_tab(id) else {
            return;
        };
        let pane = &self.groups[g].tabs[ix].pane;
        let Some(remote) = pane.open_remote.clone() else {
            return;
        };
        let dir = pane.path.clone();
        self.drop_into(dragged, remote, dir, mods, cx);
    }
}
