//! Directory navigation and history; the explorer and sidebar own their views.
//! All navigation acts on the active tab — its location, history, and explorer.

use super::*;

impl Workspace {
    /// The selected entries (operands for copy/cut/delete/download).
    pub(crate) fn selected_entries(&self, cx: &App) -> Vec<Entry> {
        self.explorer().read(cx).selected_entries()
    }

    pub(crate) fn prompt(&self) -> Option<Entity<PromptModal>> {
        self.prompt.clone()
    }

    pub(crate) fn force_reload_entries(&mut self, cx: &mut Context<Self>) {
        self.explorer().update(cx, |e, cx| e.force_reload_entries(cx));
    }

    pub(crate) fn reload(&mut self, _: &Reload, _window: &mut Window, cx: &mut Context<Self>) {
        self.force_reload_entries(cx);
    }

    pub(crate) fn toggle_search_action(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.explorer().update(cx, |e, cx| e.toggle_search(window, cx));
    }

    /// Whether the explorer pane currently holds keyboard focus.
    pub(crate) fn explorer_focused(&self, window: &Window, cx: &App) -> bool {
        self.explorer().focus_handle(cx).contains_focused(window, cx)
    }

    pub(crate) fn focus_explorer_pane(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.explorer().focus_handle(cx).focus(window, cx);
    }

    /// Deliberately move into the explorer (Tab / arrow): focus it and, if it has
    /// no selection, land the cursor on the first row.
    pub(crate) fn enter_explorer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_explorer_pane(window, cx);
        self.explorer().update(cx, |e, cx| e.select_first_if_empty(cx));
    }

    pub(crate) fn focus_sidebar_pane(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.focus_handle(cx).focus(window, cx);
    }

    pub(crate) fn go_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active().open_remote.is_none() {
            return;
        }
        let tab = self.active_mut();
        tab.open_remote = None;
        tab.path = String::new();
        tab.history.clear();
        tab.history_pos = 0;
        self.prompt = None;
        self.menus.context = None;
        self.menus.bg_menu = None;
        self.explorer().update(cx, |e, cx| e.show(None, String::new(), None, cx));
        self.focus_sidebar_pane(window, cx);
        cx.notify();
    }

    /// Push a new location onto the active tab's history, selecting `want` (by
    /// name) on arrival. Saves the current row first so going back restores it.
    pub(crate) fn navigate(&mut self, remote: String, path: String, want: Option<String>, cx: &mut Context<Self>) {
        if self.active().open_remote.as_deref() != Some(remote.as_str()) {
            self.app.db.record_remote(&remote);
            self.recent_remotes = self.app.db.recent_remotes(RECENT_REMOTES_FETCH);
        }
        // Keep the sidebar highlight on the remote being shown. Every open path
        // routes through navigate(), so syncing here covers all of them.
        self.select_remote(Some(&remote), cx);
        self.remember_sel(cx);
        self.remote_paths.insert(remote.clone(), path.clone());
        let tab = self.active_mut();
        tab.open_remote = Some(remote.clone());
        tab.path = path.clone();
        tab.history.truncate(tab.history_pos + 1);
        tab.history.push(Location { remote: remote.clone(), path: path.clone(), selected: None });
        tab.history_pos = tab.history.len() - 1;
        self.explorer().update(cx, |e, cx| e.show(Some(remote), path, want, cx));
        cx.notify();
    }

    /// Jump to the open remote's root.
    pub(crate) fn go_to_root(&mut self, cx: &mut Context<Self>) {
        if let Some(remote) = self.active().open_remote.clone() {
            self.navigate(remote, String::new(), None, cx);
        }
    }

    pub(crate) fn reveal_target(&mut self, target: JobTarget, window: &mut Window, cx: &mut Context<Self>) {
        // The Tasks dock sits beside the explorer, so revealing a target navigates
        // the explorer without closing the dock (unlike the old bottom panel).
        self.reveal_target_in_explorer(target, cx);
        self.focus_explorer_pane(window, cx);
    }

    /// Navigate the explorer to a job endpoint (no focus change — for menu actions
    /// that lack a `Window`).
    pub(crate) fn reveal_target_in_explorer(&mut self, target: JobTarget, cx: &mut Context<Self>) {
        if target.is_dir {
            self.navigate(target.remote, target.path, None, cx);
        } else {
            let containing_dir = parent_of(&target.path).to_string();
            self.navigate(target.remote, containing_dir, Some(target.name.to_string()), cx);
        }
    }

    /// Remember the cursor row of the current location, so going back restores it.
    pub(crate) fn remember_sel(&mut self, cx: &mut Context<Self>) {
        let name = self.explorer().read(cx).cursor_name();
        let tab = self.active_mut();
        let pos = tab.history_pos;
        if let Some(loc) = tab.history.get_mut(pos) {
            loc.selected = name;
        }
    }

    pub(crate) fn can_back(&self) -> bool {
        self.active().history_pos > 0
    }

    pub(crate) fn can_forward(&self) -> bool {
        let tab = self.active();
        tab.history_pos + 1 < tab.history.len()
    }

    pub(crate) fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.can_back() {
            self.remember_sel(cx);
            self.active_mut().history_pos -= 1;
            self.restore_history(cx);
        }
    }

    pub(crate) fn go_forward(&mut self, cx: &mut Context<Self>) {
        if self.can_forward() {
            self.remember_sel(cx);
            self.active_mut().history_pos += 1;
            self.restore_history(cx);
        }
    }

    pub(crate) fn restore_history(&mut self, cx: &mut Context<Self>) {
        let loc = self.active().history[self.active().history_pos].clone();
        let tab = self.active_mut();
        tab.open_remote = Some(loc.remote.clone());
        tab.path = loc.path.clone();
        self.explorer().update(cx, |e, cx| e.show(Some(loc.remote), loc.path, loc.selected, cx));
        cx.notify();
    }

    pub(crate) fn action_back(&mut self, _: &GoBack, _window: &mut Window, cx: &mut Context<Self>) {
        self.go_back(cx);
    }

    pub(crate) fn action_forward(&mut self, _: &GoForward, _window: &mut Window, cx: &mut Context<Self>) {
        self.go_forward(cx);
    }

    pub(crate) fn go_up(&mut self, _: &GoUp, window: &mut Window, cx: &mut Context<Self>) {
        if !self.explorer_focused(window, cx) {
            return;
        }
        if self.active().path.is_empty() {
            self.focus_sidebar_pane(window, cx);
            cx.notify();
        } else {
            let path = self.active().path.clone();
            let child = path.rsplit('/').next().unwrap_or_default().to_string();
            let parent = parent_of(&path).to_string();
            let remote = self.active().open_remote.clone().unwrap_or_default();
            self.navigate(remote, parent, Some(child), cx);
        }
    }
}
