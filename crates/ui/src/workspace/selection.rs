//! Entry multi-selection and open.

use super::*;

impl Workspace {
    pub(crate) fn selected_entries(&self) -> Vec<Entry> {
        // No selection means no operands — keyboard copy/cut/delete/download
        // no-op rather than silently acting on the cursor row.
        if self.selected.is_empty() {
            return Vec::new();
        }
        self.entries().iter().filter(|e| self.selected.contains(&e.path)).cloned().collect()
    }

    pub(crate) fn select_only(&mut self, ix: usize) {
        self.entry_sel = ix;
        self.sel_anchor = ix;
        self.selected.clear();
        if let Some(p) = self.entry_path_at(ix) {
            self.selected.insert(p);
        }
    }

    pub(crate) fn toggle_at(&mut self, ix: usize) {
        self.entry_sel = ix;
        self.sel_anchor = ix;
        if let Some(p) = self.entry_path_at(ix) {
            if !self.selected.remove(&p) {
                self.selected.insert(p);
            }
        }
    }

    pub(crate) fn select_range_to(&mut self, ix: usize) {
        let (lo, hi) = (self.sel_anchor.min(ix), self.sel_anchor.max(ix));
        let paths: Vec<String> = self
            .entries()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= lo && *i <= hi)
            .map(|(_, e)| e.path.clone())
            .collect();
        self.selected = paths.into_iter().collect();
        self.entry_sel = ix;
    }

    pub(crate) fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pane != Pane::Explorer {
            return;
        }
        let all: HashSet<String> = self.entries().iter().map(|e| e.path.clone()).collect();
        self.selected = all;
        cx.notify();
    }

    pub(crate) fn select_next(&mut self, _: &SelectNext, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.active_len();
        if len == 0 {
            return;
        }
        match self.pane {
            Pane::Sidebar => {
                if self.remote_sel + 1 < len {
                    self.remote_sel += 1;
                }
            }
            Pane::Explorer => {
                if self.selected.is_empty() {
                    self.select_only(0);
                } else {
                    let next = (self.entry_sel + 1).min(len - 1);
                    if window.modifiers().shift {
                        self.select_range_to(next);
                    } else {
                        self.select_only(next);
                    }
                }
            }
        }
        cx.notify();
        self.scroll_to_selection();
    }

    pub(crate) fn select_prev(&mut self, _: &SelectPrev, window: &mut Window, cx: &mut Context<Self>) {
        match self.pane {
            Pane::Sidebar => self.remote_sel = self.remote_sel.saturating_sub(1),
            Pane::Explorer => {
                let len = self.entries().len();
                if len == 0 {
                    return;
                }
                if self.selected.is_empty() {
                    self.select_only(len - 1);
                    cx.notify();
                    self.scroll_to_selection();
                    return;
                }
                let prev = self.entry_sel.saturating_sub(1);
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
        match self.pane {
            Pane::Sidebar => {
                self.load_remote(self.remote_sel, cx);
                self.pane = Pane::Explorer;
            }
            Pane::Explorer => self.descend(self.entry_sel, cx),
        }
    }

}
