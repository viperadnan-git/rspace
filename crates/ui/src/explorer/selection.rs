use super::*;

impl Explorer {
    pub(crate) fn selected_entries(&self) -> Vec<Entry> {
        if self.sel.is_empty() {
            return Vec::new();
        }
        self.entries().iter().filter(|e| self.sel.contains(&e.path)).cloned().collect()
    }

    /// The cursor row's name (used by the preview and back/forward memory).
    pub(crate) fn cursor_name(&self) -> Option<String> {
        self.entry_sel.and_then(|ix| self.entries().get(ix)).map(|e| e.name.clone())
    }

    /// The cursor row's entry — the rename target and preview subject. `None`
    /// when nothing is selected.
    pub(crate) fn cursor_entry(&self) -> Option<Entry> {
        self.entry_sel.and_then(|ix| self.entries().get(ix).cloned())
    }

    pub(crate) fn selection_len(&self) -> usize {
        self.sel.len()
    }

    /// Collapse a multi-selection back to just the cursor row.
    pub(crate) fn collapse_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self.entry_sel {
            self.select_only(ix);
            cx.notify();
        }
    }

    /// Select `name` once the next listing loads (after rename / new folder).
    pub(crate) fn set_pending(&mut self, name: String) {
        self.pending_select = Some(name);
    }

    pub(crate) fn select_only(&mut self, ix: usize) {
        match self.entries().get(ix).map(|e| e.path.clone()) {
            Some(p) => {
                self.sel.select_only(p);
                self.entry_sel = Some(ix);
            }
            None => {
                self.sel.clear();
                self.entry_sel = None;
            }
        }
    }

    pub(crate) fn toggle_at(&mut self, ix: usize) {
        if let Some(p) = self.entries().get(ix).map(|e| e.path.clone()) {
            self.sel.toggle(p);
        }
        self.entry_sel = (!self.sel.is_empty()).then_some(ix);
    }

    pub(crate) fn select_range_to(&mut self, ix: usize) {
        let order: Vec<String> = self.entries().iter().map(|e| e.path.clone()).collect();
        if let Some(p) = order.get(ix).cloned() {
            self.sel.range_to(&order, p);
        }
        self.entry_sel = (!self.sel.is_empty()).then_some(ix);
    }

    pub(crate) fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if !self.sel.is_empty() {
            self.sel.clear();
            self.entry_sel = None;
            cx.notify();
        }
    }

    /// On deliberate keyboard entry into the pane, land the cursor on the first
    /// row if nothing is selected — so the list is immediately navigable.
    pub(crate) fn select_first_if_empty(&mut self, cx: &mut Context<Self>) {
        if self.sel.is_empty() && !self.entries().is_empty() {
            self.select_only(0);
            self.scroll_to_selection();
            cx.notify();
        }
    }

    pub(crate) fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.sel.set_to(self.entries().iter().map(|e| e.path.clone()).collect());
        self.entry_sel = (!self.sel.is_empty()).then(|| self.entry_sel.unwrap_or(0));
        cx.notify();
    }

    pub(crate) fn select_next(&mut self, _: &SelectNext, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.entries().len();
        if len == 0 {
            return;
        }
        match self.entry_sel {
            None => self.select_only(0),
            Some(cur) => {
                let next = (cur + 1).min(len - 1);
                if window.modifiers().shift {
                    self.select_range_to(next);
                } else {
                    self.select_only(next);
                }
            }
        }
        cx.notify();
        self.scroll_to_selection();
    }

    pub(crate) fn select_prev(&mut self, _: &SelectPrev, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.entries().len();
        if len == 0 {
            return;
        }
        match self.entry_sel {
            None => self.select_only(len - 1),
            Some(cur) => {
                let prev = cur.saturating_sub(1);
                if window.modifiers().shift {
                    self.select_range_to(prev);
                } else {
                    self.select_only(prev);
                }
            }
        }
        cx.notify();
        self.scroll_to_selection();
    }

    pub(crate) fn open(&mut self, _: &Open, _window: &mut Window, cx: &mut Context<Self>) {
        // Only the cursor row opens; with no selection there is no cursor.
        if let Some(ix) = self.entry_sel {
            self.descend(ix, cx);
        }
    }

    /// Open a folder (navigate) or a file (select + preview) at `ix`.
    pub(crate) fn descend(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some((is_dir, path)) = self.entries().get(ix).map(|e| (e.is_dir, e.path.clone()))
        else {
            return;
        };
        if is_dir {
            cx.emit(ExplorerEvent::OpenDir(path));
        } else {
            self.select_only(ix);
            cx.emit(ExplorerEvent::OpenFile);
        }
    }

    pub(crate) fn scroll_to_selection(&self) {
        if let Some(ix) = self.entry_sel {
            self.entry_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }
}
