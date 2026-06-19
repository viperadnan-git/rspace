//! Directory navigation, history, entry cursor.

use super::*;

impl Workspace {
    pub(crate) fn entries(&self) -> &[Entry] {
        if self.recursive_showing() {
            self.search_query.data().map(Vec::as_slice).unwrap_or(&[])
        } else if self.has_query() {
            &self.view
        } else {
            self.dir_query.data().map(Vec::as_slice).unwrap_or(&[])
        }
    }

    pub(crate) fn load_entries(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        let service = self.service.clone();
        let (field, order) = (self.sort_field, self.sort_order);
        self.dir_query.load(
            (remote, self.path.clone()),
            cx,
            |this| &mut this.dir_query,
            move |(remote, path)| async move {
                let mut entries = service.list_dir(&remote, &path).await?;
                sort_entries(&mut entries, field, order);
                Ok::<_, ServiceError>(entries)
            },
        );
    }

    pub(crate) fn reload(&mut self, _: &Reload, _window: &mut Window, cx: &mut Context<Self>) {
        self.force_reload_entries(cx);
    }

    pub(crate) fn force_reload_entries(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let (field, order) = (self.sort_field, self.sort_order);
        self.dir_query.reload(cx, |this| &mut this.dir_query, move |(remote, path)| async move {
            let mut entries = service.list_dir(&remote, &path).await?;
            sort_entries(&mut entries, field, order);
            Ok::<_, ServiceError>(entries)
        });
    }

    /// Open the command palette, or close it if already open. Ignored while
    /// another modal is up (don't stack modals).
    pub(crate) fn choose_sort(&mut self, field: SortField, cx: &mut Context<Self>) {
        if self.sort_field == field {
            self.sort_order = self.sort_order.toggle();
        } else {
            self.sort_field = field;
        }
        let (field, order) = (self.sort_field, self.sort_order);
        self.store.update(|s| {
            s.sort_field = field;
            s.sort_order = order;
        });
        self.pending_select = self.entries().get(self.entry_sel).map(|e| e.name.clone());
        self.dir_query.update_current(move |entries| sort_entries(entries, field, order));
        cx.notify();
    }

    pub(crate) fn descend(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some((is_dir, path)) = self.entries().get(ix).map(|e| (e.is_dir, e.path.clone()))
        else {
            return;
        };
        if is_dir {
            let remote = self.open_remote.clone().unwrap_or_default();
            self.navigate(remote, path, None, cx);
        } else {
            self.select_only(ix);
            self.open_preview(cx);
        }
    }

    pub(crate) fn go_home(&mut self, cx: &mut Context<Self>) {
        if self.open_remote.is_none() {
            return;
        }
        self.open_remote = None;
        self.path = String::new();
        self.entry_sel = 0;
        self.sel_anchor = 0;
        self.selected.clear();
        self.pending_select = None;
        self.preview = None;
        self.prompt = None;
        self.context = None;
        self.bg_menu = None;
        self.history.clear();
        self.history_pos = 0;
        self.pane = Pane::Sidebar;
        cx.notify();
    }

    /// Push a new location onto history, selecting `want` (by name) on arrival.
    /// Saves the current row first so going back restores it.
    pub(crate) fn navigate(&mut self, remote: String, path: String, want: Option<String>, cx: &mut Context<Self>) {
        self.reset_search(cx);
        if self.open_remote.as_deref() != Some(remote.as_str()) {
            self.db.record_remote(&remote);
            self.recent_remotes = self.db.recent_remotes(RECENT_REMOTES_FETCH);
        }
        // Keep the sidebar highlight on the remote being shown. Every open path
        // routes through navigate(), so syncing here covers all of them.
        self.select_remote(Some(&remote));
        self.remember_sel();
        self.open_remote = Some(remote.clone());
        self.path = path.clone();
        self.remote_paths.insert(remote.clone(), path.clone());
        self.history.truncate(self.history_pos + 1);
        self.history.push(Location { remote, path, selected: None });
        self.history_pos = self.history.len() - 1;
        self.entry_sel = 0;
        self.sel_anchor = 0;
        self.selected.clear();
        self.pending_select = want;
        self.load_entries(cx);
    }

    pub(crate) fn reveal_target(&mut self, target: JobTarget, cx: &mut Context<Self>) {
        self.jobs_open = false;
        self.pane = Pane::Explorer;
        if target.is_dir {
            self.navigate(target.remote, target.path, None, cx);
        } else {
            let containing_dir = parent_of(&target.path).to_string();
            self.navigate(target.remote, containing_dir, Some(target.name.to_string()), cx);
        }
    }

    /// Resize a file-list column by dragging its left divider. Width is measured
    /// from the table's right content edge (the Name column flex-grows to fill).
    pub(crate) fn remember_sel(&mut self) {
        let name = self.entries().get(self.entry_sel).map(|e| e.name.clone());
        if let Some(loc) = self.history.get_mut(self.history_pos) {
            loc.selected = name;
        }
    }

    pub(crate) fn can_back(&self) -> bool {
        self.history_pos > 0
    }

    pub(crate) fn can_forward(&self) -> bool {
        self.history_pos + 1 < self.history.len()
    }

    pub(crate) fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.can_back() {
            self.remember_sel();
            self.history_pos -= 1;
            self.restore_history(cx);
        }
    }

    pub(crate) fn go_forward(&mut self, cx: &mut Context<Self>) {
        if self.can_forward() {
            self.remember_sel();
            self.history_pos += 1;
            self.restore_history(cx);
        }
    }

    pub(crate) fn restore_history(&mut self, cx: &mut Context<Self>) {
        let loc = self.history[self.history_pos].clone();
        self.open_remote = Some(loc.remote);
        self.path = loc.path;
        self.pane = Pane::Explorer;
        self.entry_sel = 0;
        self.sel_anchor = 0;
        self.selected.clear();
        self.pending_select = loc.selected;
        self.load_entries(cx);
    }

    /// Apply a pending select-by-name once its listing has loaded, then clamp.
    /// A freshly opened directory has *no* selection (Finder-style) — only an
    /// explicit `pending_select` (e.g. after rename, or the child folder when
    /// going up) selects an item.
    pub(crate) fn resolve_selection(&mut self) {
        if self.dir_query.data().is_none() {
            return;
        }
        if let Some(name) = self.pending_select.take() {
            let idx = self.entries().iter().position(|e| e.name == name);
            if let Some(idx) = idx {
                self.select_only(idx);
                self.scroll_to_selection();
                return;
            }
        }
        let len = self.entries().len();
        if len > 0 && self.entry_sel >= len {
            self.entry_sel = len - 1;
        }
        if !self.selected.is_empty() {
            let valid: HashSet<String> = self.entries().iter().map(|e| e.path.clone()).collect();
            self.selected.retain(|p| valid.contains(p));
        }
    }

    pub(crate) fn action_back(&mut self, _: &GoBack, _window: &mut Window, cx: &mut Context<Self>) {
        self.go_back(cx);
    }

    pub(crate) fn action_forward(&mut self, _: &GoForward, _window: &mut Window, cx: &mut Context<Self>) {
        self.go_forward(cx);
    }

    pub(crate) fn active_len(&self) -> usize {
        match self.pane {
            Pane::Sidebar => self.remotes.len(),
            Pane::Explorer => self.entries().len(),
        }
    }

    pub(crate) fn scroll_to_selection(&self) {
        match self.pane {
            Pane::Sidebar => {
                self.remote_scroll.scroll_to_item(self.remote_sel, ScrollStrategy::Nearest)
            }
            Pane::Explorer => {
                self.entry_scroll.scroll_to_item(self.entry_sel, ScrollStrategy::Nearest)
            }
        }
    }

    pub(crate) fn entry_path_at(&self, ix: usize) -> Option<String> {
        self.entries().get(ix).map(|e| e.path.clone())
    }

    pub(crate) fn go_up(&mut self, _: &GoUp, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pane != Pane::Explorer {
            return;
        }
        if self.path.is_empty() {
            self.pane = Pane::Sidebar;
            cx.notify();
        } else {
            let child = self.path.rsplit('/').next().unwrap_or_default().to_string();
            let parent = parent_of(&self.path).to_string();
            let remote = self.open_remote.clone().unwrap_or_default();
            self.navigate(remote, parent, Some(child), cx);
        }
    }

}
