//! Tab lifecycle: open, close, and switch browse-context tabs. Each tab is an
//! independent [`Tab`]; the active one is rendered. Splitting these tabs across
//! side-by-side panes later only needs a `Pane` wrapper around `tabs`/`active`.

use super::*;

impl Workspace {
    /// A tab's strip label: the open folder, the remote at its root, or "New Tab".
    pub(crate) fn tab_title(&self, tab: &Tab) -> String {
        match &tab.open_remote {
            None => "New Tab".to_string(),
            Some(remote) if tab.path.is_empty() => remote.clone(),
            Some(_) => tab.path.rsplit('/').next().unwrap_or_default().to_string(),
        }
    }

    /// Open a new tab on the welcome screen and focus it. Inherits the active
    /// column widths so the new pane matches what's on screen.
    pub(crate) fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let (sort, refresh_secs) = {
            let s = self.store.get();
            ((s.sort_field, s.sort_order), s.refresh_secs)
        };
        let cols = {
            let e = self.active().explorer.read(cx);
            (e.col_date_width(), e.col_size_width())
        };
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = Self::build_tab(id, &weak, &self.app.service, sort, refresh_secs, cols, window, cx);
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.set_active_polling(cx);
        self.retarget_preview(cx);
        let explorer = self.active().explorer.clone();
        self.path_bar.update(cx, |pb, cx| pb.set_explorer(explorer, cx));
        // A fresh tab opens on the welcome screen, so clear the sidebar highlight.
        self.select_remote(None, cx);
        self.close_menus();
        self.focus_active_tab(window, cx);
        cx.notify();
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
        self.close_tab_at(self.active, window, cx);
    }

    /// Close tab `ix`. The last remaining tab is reset to the welcome screen
    /// rather than closing the window.
    pub(crate) fn close_tab_at(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        if self.tabs.len() == 1 {
            self.go_home(window, cx);
            return;
        }
        self.tabs.remove(ix);
        if ix < self.active {
            self.active -= 1;
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        self.sync_to_active_tab(window, cx);
    }

    pub(crate) fn select_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.tabs.len() && ix != self.active {
            self.active = ix;
            self.sync_to_active_tab(window, cx);
        }
    }

    pub(crate) fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.tabs.len();
        if n > 1 {
            self.select_tab((self.active + 1) % n, window, cx);
        }
    }

    pub(crate) fn prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.tabs.len();
        if n > 1 {
            self.select_tab((self.active + n - 1) % n, window, cx);
        }
    }

    /// After the active tab changes: re-sync polling, the preview target, the
    /// sidebar highlight, and focus.
    fn sync_to_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_active_polling(cx);
        self.retarget_preview(cx);
        let explorer = self.active().explorer.clone();
        self.path_bar.update(cx, |pb, cx| pb.set_explorer(explorer, cx));
        let remote = self.active().open_remote.clone();
        self.select_remote(remote.as_deref(), cx);
        self.close_menus();
        self.focus_active_tab(window, cx);
        cx.notify();
    }

    /// Activate only the current tab's explorer; background tabs stop polling.
    fn set_active_polling(&mut self, cx: &mut Context<Self>) {
        for (ix, tab) in self.tabs.iter().enumerate() {
            let active = ix == self.active;
            tab.explorer.update(cx, |e, cx| e.set_active(active, cx));
        }
    }

    /// Route focus to the active tab's natural pane: the explorer when a remote is
    /// open, otherwise the sidebar (welcome screen). Focuses the pane but does not
    /// invent a selection — a tab deliberately left with no cursor keeps it.
    fn focus_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active().open_remote.is_some() {
            self.focus_explorer_pane(window, cx);
        } else {
            self.focus_sidebar_pane(window, cx);
        }
    }

    fn tab_index(&self, id: usize) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn active_id(&self) -> usize {
        self.tabs[self.active].id
    }

    pub(crate) fn is_tab_pinned(&self, id: usize) -> bool {
        self.tab_index(id).is_some_and(|ix| self.tabs[ix].pinned)
    }

    /// Re-point `active` at the tab with `id`, clamping if it's gone.
    fn set_active_id(&mut self, id: usize) {
        self.active =
            self.tab_index(id).unwrap_or_else(|| self.active.min(self.tabs.len().saturating_sub(1)));
    }

    /// Pin/unpin a tab, regrouping so pinned tabs stay at the front (stable order
    /// within each group). The active tab follows the move.
    pub(crate) fn toggle_pin_tab(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(ix) = self.tab_index(id) else { return };
        let active_id = self.active_id();
        self.tabs[ix].pinned = !self.tabs[ix].pinned;
        let tab = self.tabs.remove(ix);
        // The boundary between the pinned and unpinned groups: end of pins when
        // pinning, start of the unpinned run when unpinning — same index.
        let dest = self.tabs.iter().take_while(|t| t.pinned).count();
        self.tabs.insert(dest, tab);
        self.set_active_id(active_id);
        cx.notify();
    }

    /// Close a specific tab by id (context menu — closes even a pinned tab).
    pub(crate) fn close_tab_id(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.tab_index(id) {
            self.close_tab_at(ix, window, cx);
        }
    }

    /// Close every tab but `id` and the pinned ones.
    pub(crate) fn close_other_tabs(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let keep: Vec<usize> =
            self.tabs.iter().filter(|t| t.id == id || t.pinned).map(|t| t.id).collect();
        self.retain_tabs(&keep, id, window, cx);
    }

    /// Close unpinned tabs to the right of `id`.
    pub(crate) fn close_tabs_to_right(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.tab_index(id) else { return };
        let keep: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(ix, t)| *ix <= target || t.pinned)
            .map(|(_, t)| t.id)
            .collect();
        self.retain_tabs(&keep, id, window, cx);
    }

    /// Close all unpinned tabs (pinned tabs stay).
    pub(crate) fn close_all_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keep: Vec<usize> = self.tabs.iter().filter(|t| t.pinned).map(|t| t.id).collect();
        let prefer = keep.first().copied().unwrap_or_else(|| self.active_id());
        self.retain_tabs(&keep, prefer, window, cx);
    }

    /// Drop every tab whose id isn't in `keep`, then activate `prefer`. An empty
    /// `keep` collapses to a single welcome tab (the window never closes). `keep`
    /// holds a handful of ids, so a linear `contains` beats allocating a set.
    fn retain_tabs(&mut self, keep: &[usize], prefer: usize, window: &mut Window, cx: &mut Context<Self>) {
        if keep.is_empty() {
            let active_id = self.active_id();
            self.tabs.retain(|t| t.id == active_id);
            self.active = 0;
            self.go_home(window, cx);
            self.set_active_polling(cx);
            return;
        }
        if keep.len() == self.tabs.len() {
            return;
        }
        self.tabs.retain(|t| keep.contains(&t.id));
        self.set_active_id(prefer);
        self.sync_to_active_tab(window, cx);
    }

    /// Jump to tab `n` (1-based); `n == 9` lands on the last tab (browser-style).
    pub(crate) fn jump_to_tab(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let last = self.tabs.len().saturating_sub(1);
        let ix = if n >= 9 { last } else { (n - 1).min(last) };
        self.select_tab(ix, window, cx);
    }

    /// Move the dragged tab to the dropped-on tab's slot. Pinned tabs stay grouped
    /// at the front, so a tab only reorders within its own (pinned/unpinned) group.
    pub(crate) fn reorder_tab(&mut self, from_id: usize, to_id: usize, cx: &mut Context<Self>) {
        if from_id == to_id {
            return;
        }
        let (Some(from), Some(to)) = (self.tab_index(from_id), self.tab_index(to_id)) else {
            return;
        };
        if self.tabs[from].pinned != self.tabs[to].pinned {
            return;
        }
        let active_id = self.active_id();
        let tab = self.tabs.remove(from);
        let dest = if from < to { to - 1 } else { to };
        self.tabs.insert(dest, tab);
        self.set_active_id(active_id);
        cx.notify();
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
                    if let Some(ix) = this.tab_index(id) {
                        this.select_tab(ix, window, cx);
                    }
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
        let Some(ix) = self.tab_index(id) else {
            return;
        };
        let Some(remote) = self.tabs[ix].open_remote.clone() else {
            return;
        };
        let dir = self.tabs[ix].path.clone();
        self.drop_into(dragged, remote, dir, mods, cx);
    }
}
