//! Directory navigation and history; the explorer owns the listing/selection.

use super::*;

impl Workspace {
    /// The selected entries (operands for copy/cut/delete/download).
    pub(crate) fn selected_entries(&self, cx: &App) -> Vec<Entry> {
        self.explorer.read(cx).selected_entries()
    }

    pub(crate) fn prompt(&self) -> Option<Entity<PromptModal>> {
        self.prompt.clone()
    }

    pub(crate) fn force_reload_entries(&mut self, cx: &mut Context<Self>) {
        self.explorer.update(cx, |e, cx| e.force_reload_entries(cx));
    }

    pub(crate) fn toggle_search_action(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.explorer.update(cx, |e, cx| e.toggle_search(window, cx));
    }

    pub(crate) fn reload(&mut self, _: &Reload, _window: &mut Window, cx: &mut Context<Self>) {
        self.force_reload_entries(cx);
    }

    pub(crate) fn go_home(&mut self, cx: &mut Context<Self>) {
        if self.open_remote.is_none() {
            return;
        }
        self.open_remote = None;
        self.path = String::new();
        self.preview = None;
        self.prompt = None;
        self.context = None;
        self.bg_menu = None;
        self.history.clear();
        self.history_pos = 0;
        self.pane = Pane::Sidebar;
        self.explorer.update(cx, |e, cx| e.show(None, String::new(), None, cx));
        cx.notify();
    }

    /// Push a new location onto history, selecting `want` (by name) on arrival.
    /// Saves the current row first so going back restores it.
    pub(crate) fn navigate(&mut self, remote: String, path: String, want: Option<String>, cx: &mut Context<Self>) {
        if self.open_remote.as_deref() != Some(remote.as_str()) {
            self.db.record_remote(&remote);
            self.recent_remotes = self.db.recent_remotes(RECENT_REMOTES_FETCH);
        }
        // Keep the sidebar highlight on the remote being shown. Every open path
        // routes through navigate(), so syncing here covers all of them.
        self.select_remote(Some(&remote));
        self.remember_sel(cx);
        self.open_remote = Some(remote.clone());
        self.path = path.clone();
        self.remote_paths.insert(remote.clone(), path.clone());
        self.history.truncate(self.history_pos + 1);
        self.history.push(Location { remote: remote.clone(), path: path.clone(), selected: None });
        self.history_pos = self.history.len() - 1;
        self.explorer.update(cx, |e, cx| e.show(Some(remote), path, want, cx));
        cx.notify();
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

    /// Remember the cursor row of the current location, so going back restores it.
    pub(crate) fn remember_sel(&mut self, cx: &mut Context<Self>) {
        let name = self.explorer.read(cx).cursor_name();
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
            self.remember_sel(cx);
            self.history_pos -= 1;
            self.restore_history(cx);
        }
    }

    pub(crate) fn go_forward(&mut self, cx: &mut Context<Self>) {
        if self.can_forward() {
            self.remember_sel(cx);
            self.history_pos += 1;
            self.restore_history(cx);
        }
    }

    pub(crate) fn restore_history(&mut self, cx: &mut Context<Self>) {
        let loc = self.history[self.history_pos].clone();
        self.open_remote = Some(loc.remote.clone());
        self.path = loc.path.clone();
        self.pane = Pane::Explorer;
        self.explorer.update(cx, |e, cx| e.show(Some(loc.remote), loc.path, loc.selected, cx));
        cx.notify();
    }

    pub(crate) fn action_back(&mut self, _: &GoBack, _window: &mut Window, cx: &mut Context<Self>) {
        self.go_back(cx);
    }

    pub(crate) fn action_forward(&mut self, _: &GoForward, _window: &mut Window, cx: &mut Context<Self>) {
        self.go_forward(cx);
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

    // --- sidebar (remote list) keyboard nav; the explorer owns its own ---------

    pub(crate) fn select_next(&mut self, _: &SelectNext, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pane != Pane::Sidebar {
            return;
        }
        let len = self.remotes.len();
        if len > 0 && self.remote_sel + 1 < len {
            self.remote_sel += 1;
            self.remote_scroll.scroll_to_item(self.remote_sel, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    pub(crate) fn select_prev(&mut self, _: &SelectPrev, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pane != Pane::Sidebar {
            return;
        }
        self.remote_sel = self.remote_sel.saturating_sub(1);
        self.remote_scroll.scroll_to_item(self.remote_sel, ScrollStrategy::Nearest);
        cx.notify();
    }

    pub(crate) fn open(&mut self, _: &Open, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pane == Pane::Sidebar {
            self.load_remote(self.remote_sel, cx);
            self.pane = Pane::Explorer;
        }
    }
}
