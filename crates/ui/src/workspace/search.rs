//! Recursive / in-folder search.

use super::*;

impl Workspace {
    pub(crate) fn has_query(&self) -> bool {
        self.search.split_whitespace().next().is_some()
    }

    pub(crate) fn recursive_intent(&self) -> bool {
        self.searched.as_deref() == Some(self.search.as_str())
    }

    pub(crate) fn recursive_showing(&self) -> bool {
        self.recursive_intent() && self.search_query.data().is_some()
    }

    /// Per-frame; skips rebuild when query and dir entries are unchanged.
    pub(crate) fn rebuild_search_view(&mut self) {
        if self.recursive_showing() || !self.has_query() {
            return;
        }
        let dir_len = self.dir_query.data().map_or(0, |v| v.len());
        if self.view_sig.as_ref().is_some_and(|(q, n)| q == &self.search && *n == dir_len) {
            return;
        }
        let matcher = Matcher::new(&self.search);
        self.view = self
            .dir_query
            .data()
            .map(|es| es.iter().filter(|e| matcher.matches(&e.name)).cloned().collect())
            .unwrap_or_default();
        self.view_sig = Some((self.search.clone(), dir_len));
    }

    pub(crate) fn search_submit(&mut self, _: &SearchSubmit, _: &mut Window, cx: &mut Context<Self>) {
        self.run_search(cx);
    }

    pub(crate) fn toggle_subfolder_search(&mut self, cx: &mut Context<Self>) {
        if self.recursive_intent() {
            self.searched = None;
            cx.notify();
        } else {
            self.run_search(cx);
        }
    }

    pub(crate) fn run_search(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        let query = self.search.trim().to_string();
        if query.is_empty() {
            self.searched = None;
            return;
        }
        self.searched = Some(self.search.clone());
        let path = self.path.clone();
        let service = self.service.clone();
        let (field, order) = (self.sort_field, self.sort_order);
        self.search_query.load(
            (remote, path, query),
            cx,
            |this| &mut this.search_query,
            move |(remote, path, query)| async move {
                let mut entries = service.search(&remote, &path, &query).await?;
                sort_entries(&mut entries, field, order);
                Ok::<_, ServiceError>(entries)
            },
        );
    }

    pub(crate) fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = !self.search_open;
        if self.search_open {
            self.search_input.read(cx).focus_handle(cx).focus(window, cx);
        } else {
            self.reset_search(cx);
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn toggle_search_action(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_search(window, cx);
    }

    pub(crate) fn close_search(&mut self, _: &CloseSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open {
            self.search_open = false;
            self.reset_search(cx);
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    pub(crate) fn clear_search(&mut self, cx: &mut Context<Self>) {
        self.searched = None;
        self.search.clear();
        self.search_input.update(cx, |i, cx| i.set_text(String::new(), cx));
        cx.notify();
    }

    pub(crate) fn reset_search(&mut self, cx: &mut Context<Self>) {
        self.search_open = false;
        self.searched = None;
        self.view_sig = None;
        if !self.search.is_empty() {
            self.search.clear();
            self.search_input.update(cx, |i, cx| i.set_text(String::new(), cx));
        }
    }

}
