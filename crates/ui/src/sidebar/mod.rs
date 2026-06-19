//! The remotes sidebar as a focusable child view (Zed `Pane`-style). Owns the
//! cursor, scroll, and focus, and renders the remote list; the remotes model
//! and its operations stay on the [`Workspace`], reached through [`SidebarEvent`].

use gpui::{EventEmitter, WeakEntity};

use super::*;

mod view;

/// Signals to the owning [`Workspace`] (handled after the sidebar update).
pub(crate) enum SidebarEvent {
    /// Open the remote at this index into the ordered list.
    Open(usize),
    /// Right-click a remote: open its menu at the cursor.
    Menu(String, Point<Pixels>),
    /// The "+" header button: start the add-remote flow.
    Add,
    /// A pinned remote was dragged onto another — reorder.
    Reorder { from: String, before: String },
    /// An explorer entry was dropped onto a remote — move it to that root.
    DropEntry { dragged: DraggedEntry, dst_remote: String },
}

pub(crate) struct Sidebar {
    workspace: WeakEntity<Workspace>,
    focus: FocusHandle,
    /// Cursor row into the ordered remote list (drives the highlight).
    remote_sel: usize,
    remote_scroll: UniformListScrollHandle,
}

impl EventEmitter<SidebarEvent> for Sidebar {}

impl Focusable for Sidebar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Sidebar {
    pub(crate) fn new(workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> Self {
        Self {
            workspace,
            focus: cx.focus_handle(),
            remote_sel: 0,
            remote_scroll: UniformListScrollHandle::new(),
        }
    }

    /// The remote list in display order (pinned first), read from the workspace.
    fn ordered(&self, cx: &App) -> Vec<RemoteInfo> {
        self.workspace.upgrade().map(|ws| ws.read(cx).ordered_remotes()).unwrap_or_default()
    }

    /// Move the highlight onto `name` (no-op if it isn't listed). The caller
    /// passes the current ordered list so this never re-reads the workspace
    /// while the workspace is mid-update.
    pub(crate) fn select_by_name(&mut self, name: Option<&str>, ordered: &[RemoteInfo]) {
        if let Some(name) = name {
            if let Some(ix) = ordered.iter().position(|r| r.name == name) {
                self.remote_sel = ix;
            }
        }
    }

    pub(crate) fn selected_name(&self, ordered: &[RemoteInfo]) -> Option<String> {
        ordered.get(self.remote_sel).map(|r| r.name.clone())
    }

    fn scroll_to_cursor(&self) {
        self.remote_scroll.scroll_to_item(self.remote_sel, ScrollStrategy::Nearest);
    }

    pub(crate) fn select_next(&mut self, _: &SelectNext, _window: &mut Window, cx: &mut Context<Self>) {
        let len = self.ordered(cx).len();
        if len > 0 && self.remote_sel + 1 < len {
            self.remote_sel += 1;
            self.scroll_to_cursor();
            cx.notify();
        }
    }

    pub(crate) fn select_prev(&mut self, _: &SelectPrev, _window: &mut Window, cx: &mut Context<Self>) {
        self.remote_sel = self.remote_sel.saturating_sub(1);
        self.scroll_to_cursor();
        cx.notify();
    }

    pub(crate) fn open(&mut self, _: &Open, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_explorer(window, cx);
        cx.emit(SidebarEvent::Open(self.remote_sel));
    }

    /// Hand keyboard focus to the explorer pane (used when opening a remote).
    fn focus_explorer(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ws) = self.workspace.upgrade() {
            let handle = ws.read(cx).explorer.focus_handle(cx);
            handle.focus(window, cx);
        }
    }
}
