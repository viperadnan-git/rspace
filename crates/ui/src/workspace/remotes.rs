//! Remote list: loading, pinning, ordering, deletion.

use super::*;

impl Workspace {
    pub(crate) fn load_remotes(&self, cx: &mut Context<Self>) {
        let service = self.app.service.clone();
        cx.spawn(async move |this, cx| {
            let result = service.remotes().await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(remotes) => this.remotes = remotes,
                    Err(e) => this.toast(format!("Couldn't load remotes: {e}"), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn load_remote(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(remote) = self.ordered_remotes().get(ix) {
            let name = remote.name.clone();
            let path = self.remote_paths.get(&name).cloned().unwrap_or_default();
            self.navigate(name, path, None, cx);
        }
    }

    pub(crate) fn is_pinned(&self, name: &str) -> bool {
        self.pinned.iter().any(|n| n == name)
    }

    /// Confirm, then remove a remote from the rclone config (files untouched).
    pub(crate) fn request_delete_remote(&mut self, name: String, cx: &mut Context<Self>) {
        let shown = name.clone();
        self.ask_confirm(
            "Delete remote?",
            format!(
                "Remove \u{201c}{shown}\u{201d} from the rclone config. Files on the remote are not deleted."
            ),
            "Delete",
            true,
            move |this, cx| this.delete_remote(name, cx),
            cx,
        );
    }

    pub(crate) fn delete_remote(&mut self, name: String, cx: &mut Context<Self>) {
        let service = self.app.service.clone();
        cx.spawn(async move |this, cx| {
            let result = service.config_delete(name.clone()).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        if this.open_remote.as_deref() == Some(name.as_str()) {
                            this.open_remote = None;
                            this.path = String::new();
                        }
                        this.remote_paths.remove(&name);
                        this.pinned.retain(|n| n != &name);
                        this.app.db.save_pinned(&this.pinned);
                        this.load_remotes(cx);
                    }
                    Err(e) => this.toast(format!("Couldn't delete \"{name}\": {e}"), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Pinned remotes (in pinned order), then the rest in their existing sort.
    pub(crate) fn pinned_remotes(&self) -> Vec<RemoteInfo> {
        self.pinned
            .iter()
            .filter_map(|n| self.remotes.iter().find(|r| &r.name == n).cloned())
            .collect()
    }

    pub(crate) fn unpinned_remotes(&self) -> Vec<RemoteInfo> {
        self.remotes.iter().filter(|r| !self.is_pinned(&r.name)).cloned().collect()
    }

    pub(crate) fn ordered_remotes(&self) -> Vec<RemoteInfo> {
        let mut v = self.pinned_remotes();
        v.extend(self.unpinned_remotes());
        v
    }

    /// Number of pinned rows shown (only pins that exist in the remote list).
    pub(crate) fn pinned_count(&self) -> usize {
        self.pinned_remotes().len()
    }

    pub(crate) fn has_open_remote(&self) -> bool {
        self.open_remote.is_some()
    }

    pub(crate) fn mounted_set(&self) -> HashSet<String> {
        self.mounted.clone()
    }

    fn sidebar_cursor_name(&self, cx: &App) -> Option<String> {
        self.sidebar.read(cx).selected_name(&self.ordered_remotes())
    }

    pub(crate) fn toggle_pin(&mut self, name: String, cx: &mut Context<Self>) {
        let selected = self.sidebar_cursor_name(cx);
        match self.pinned.iter().position(|n| n == &name) {
            Some(pos) => {
                self.pinned.remove(pos);
            }
            None => self.pinned.push(name.clone()),
        }
        self.app.db.save_pinned(&self.pinned);
        self.select_remote(selected.as_deref(), cx);
        cx.notify();
    }

    pub(crate) fn reorder_pinned(&mut self, from: &str, before: &str, cx: &mut Context<Self>) {
        if from == before {
            return;
        }
        let selected = self.sidebar_cursor_name(cx);
        if let Some(fp) = self.pinned.iter().position(|n| n == from) {
            let name = self.pinned.remove(fp);
            let ip = self.pinned.iter().position(|n| n == before).unwrap_or(self.pinned.len());
            self.pinned.insert(ip, name);
            self.app.db.save_pinned(&self.pinned);
        }
        self.select_remote(selected.as_deref(), cx);
        cx.notify();
    }

    pub(crate) fn move_pinned(&mut self, name: &str, up: bool, cx: &mut Context<Self>) {
        let selected = self.sidebar_cursor_name(cx);
        if let Some(i) = self.pinned.iter().position(|n| n == name) {
            let j = if up { i.checked_sub(1) } else { (i + 1 < self.pinned.len()).then_some(i + 1) };
            if let Some(j) = j {
                self.pinned.swap(i, j);
                self.app.db.save_pinned(&self.pinned);
            }
        }
        self.select_remote(selected.as_deref(), cx);
        cx.notify();
    }

    /// Move the sidebar cursor/highlight onto `name` (no-op if it isn't listed).
    /// The highlight is derived from this by-name, so every path that opens or
    /// reorders remotes routes through here — the selection can't drift from the
    /// open remote.
    pub(crate) fn select_remote(&mut self, name: Option<&str>, cx: &mut Context<Self>) {
        let ordered = self.ordered_remotes();
        self.sidebar.update(cx, |s, _| s.select_by_name(name, &ordered));
    }

    pub(crate) fn active_remote(&self) -> Option<&RemoteInfo> {
        let name = self.open_remote.as_ref()?;
        self.remotes.iter().find(|r| &r.name == name)
    }

}
