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

    /// Invalidate `dirs` in every pane's explorer: a job-touched dir may be open in
    /// more than one tab/group, so any showing it refetches and the rest drop their
    /// stale cache.
    pub(crate) fn invalidate_dirs(&mut self, dirs: &[(String, String)], cx: &mut Context<Self>) {
        let explorers: Vec<Entity<Explorer>> = self
            .groups
            .iter()
            .flat_map(|g| g.tabs.iter().map(|t| t.pane.read(cx).explorer.clone()))
            .collect();
        for explorer in explorers {
            explorer.update(cx, |e, cx| {
                for (remote, dir) in dirs {
                    e.invalidate_dir(remote, dir, cx);
                }
            });
        }
    }

    pub(crate) fn reload(&mut self, _: &Reload, _window: &mut Window, cx: &mut Context<Self>) {
        self.force_reload_entries(cx);
    }

    pub(crate) fn toggle_search_action(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.action_bar().update(cx, |ab, cx| ab.toggle_search(window, cx));
    }

    /// Open the directory-actions menu (New folder / Upload / Paste / Copy path)
    /// at `pos` — the action bar's `+` button and the empty-space right-click share
    /// this background menu.
    pub(crate) fn open_actions_menu(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let spec = self.bg_menu_spec(cx);
        self.open_menu(spec, pos, cx);
    }

    /// Open the task context menu over `ids` (the TasksPane's selection) at `pos`.
    pub(crate) fn open_task_menu(&mut self, ids: Vec<usize>, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let spec = self.task_menu_spec(ids, cx);
        self.open_menu(spec, pos, cx);
    }

    /// Whether the explorer pane currently holds keyboard focus.
    pub(crate) fn explorer_focused(&self, window: &Window, cx: &App) -> bool {
        self.explorer().focus_handle(cx).contains_focused(window, cx)
    }

    /// Whether any focusable surface owns the keyboard — the registry the
    /// focus-restore guard consults. Add a new pane's handle here rather than
    /// growing the guard with another special case. The search field is listed
    /// explicitly because it lives on the action bar, outside the explorer subtree.
    pub(crate) fn any_pane_focused(&self, window: &Window, cx: &App) -> bool {
        let mut handles =
            vec![self.sidebar.focus_handle(cx), self.tasks.focus_handle(cx), self.focus.clone()];
        // Either group's visible pane can own the keyboard, plus its search field
        // (which lives on the action bar, outside the explorer subtree).
        for group in &self.groups {
            let pane = group.active_tab().pane.read(cx);
            handles.push(pane.explorer.focus_handle(cx));
            handles.push(pane.explorer.read(cx).search_input().focus_handle(cx));
        }
        handles.iter().any(|h| h.contains_focused(window, cx))
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
        if self.open_remote(cx).is_none() {
            return;
        }
        self.focused_pane().update(cx, |pane, cx| {
            pane.open_remote = None;
            pane.path = String::new();
            pane.history.clear();
            pane.history_pos = 0;
            pane.explorer.update(cx, |e, cx| e.show(None, String::new(), None, cx));
        });
        self.prompt = None;
        self.menu = None;
        self.focus_sidebar_pane(window, cx);
        cx.notify();
    }

    /// Push a new location onto the active tab's history, selecting `want` (by
    /// name) on arrival. Saves the current row first so going back restores it.
    pub(crate) fn navigate(&mut self, remote: String, path: String, want: Option<String>, cx: &mut Context<Self>) {
        if self.open_remote(cx).as_deref() != Some(remote.as_str()) {
            self.app.db.record_remote(&remote);
            self.frequent_remotes = self.app.db.frequent_remotes(FREQUENT_REMOTES_FETCH);
        }
        // Keep the sidebar highlight on the remote being shown. Every open path
        // routes through navigate(), so syncing here covers all of them.
        self.select_remote(Some(&remote), cx);
        self.remember_sel(cx);
        self.remote_paths.insert(remote.clone(), path.clone());
        self.focused_pane().update(cx, |pane, cx| {
            pane.open_remote = Some(remote.clone());
            pane.path = path.clone();
            pane.history.truncate(pane.history_pos + 1);
            pane.history.push(Location { remote: remote.clone(), path: path.clone(), selected: None });
            pane.history_pos = pane.history.len() - 1;
            pane.explorer.update(cx, |e, cx| e.show(Some(remote), path, want, cx));
        });
        cx.notify();
    }

    pub(crate) fn reveal_target(&mut self, target: JobTarget, window: &mut Window, cx: &mut Context<Self>) {
        // The Tasks dock sits beside the explorer, so revealing a target can
        // navigate without closing it.
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
        self.focused_pane().update(cx, |pane, _| {
            let pos = pane.history_pos;
            if let Some(loc) = pane.history.get_mut(pos) {
                loc.selected = name;
            }
        });
    }

    pub(crate) fn can_back(&self, cx: &App) -> bool {
        self.focused_pane().read(cx).history_pos > 0
    }

    pub(crate) fn can_forward(&self, cx: &App) -> bool {
        let pane = self.focused_pane();
        let pane = pane.read(cx);
        pane.history_pos + 1 < pane.history.len()
    }

    pub(crate) fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.can_back(cx) {
            self.remember_sel(cx);
            self.focused_pane().update(cx, |pane, _| pane.history_pos -= 1);
            self.restore_history(cx);
        }
    }

    pub(crate) fn go_forward(&mut self, cx: &mut Context<Self>) {
        if self.can_forward(cx) {
            self.remember_sel(cx);
            self.focused_pane().update(cx, |pane, _| pane.history_pos += 1);
            self.restore_history(cx);
        }
    }

    pub(crate) fn restore_history(&mut self, cx: &mut Context<Self>) {
        self.focused_pane().update(cx, |pane, cx| {
            let loc = pane.history[pane.history_pos].clone();
            pane.open_remote = Some(loc.remote.clone());
            pane.path = loc.path.clone();
            pane.explorer.update(cx, |e, cx| e.show(Some(loc.remote), loc.path, loc.selected, cx));
        });
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
        let path = self.open_path(cx);
        if path.is_empty() {
            self.focus_sidebar_pane(window, cx);
            cx.notify();
        } else {
            let child = path.rsplit('/').next().unwrap_or_default().to_string();
            let parent = parent_of(&path).to_string();
            let remote = self.open_remote(cx).unwrap_or_default();
            self.navigate(remote, parent, Some(child), cx);
        }
    }
}
