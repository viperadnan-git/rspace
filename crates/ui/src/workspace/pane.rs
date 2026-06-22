//! A single browse pane (the [`Explorer`] listing plus its [`ActionBar`] and the
//! location/history it's browsing) and a [`PaneGroup`] — a tab strip with its own
//! tabs. The workspace holds one group, or two side by side when split (Zed-style:
//! each side is a full browser with independent tabs).

use super::*;

pub(crate) struct Pane {
    pub(crate) explorer: Entity<Explorer>,
    pub(crate) action_bar: Entity<ActionBar>,
    _explorer_sub: gpui::Subscription,
    pub(crate) open_remote: Option<String>,
    /// Empty = root.
    pub(crate) path: String,
    pub(crate) history: Vec<Location>,
    pub(crate) history_pos: usize,
}

impl Pane {
    pub(crate) fn new(
        weak: &WeakEntity<Workspace>,
        service: &Service,
        sort: (SortField, SortOrder),
        refresh_secs: u64,
        cols: (Pixels, Pixels),
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Self {
        let explorer = cx.new(|cx| {
            Explorer::new(weak.clone(), service.clone(), sort, refresh_secs, cols, window, cx)
        });
        let explorer_sub = cx.subscribe(&explorer, Workspace::on_explorer_event);
        let action_bar = cx.new(|cx| ActionBar::new(weak.clone(), explorer.clone(), cx));
        Self {
            explorer,
            action_bar,
            _explorer_sub: explorer_sub,
            open_remote: None,
            path: String::new(),
            history: Vec::new(),
            history_pos: 0,
        }
    }
}

/// One side of the workspace: its own tab strip and tabs. The workspace holds a
/// single group, or two side by side when split.
pub(crate) struct PaneGroup {
    pub(crate) tabs: Vec<Tab>,
    /// Index of the active tab. Private: writes go through `set_active`/`clamp_active`
    /// so it stays in range; the focus re-sync that must follow a change is the
    /// workspace's job (`Workspace::active_context_changed`).
    active: usize,
    /// Horizontal scroll of this group's tab strip (persists across frames).
    pub(crate) tab_scroll: ScrollHandle,
}

impl PaneGroup {
    pub(crate) fn new(tab: Tab) -> Self {
        Self { tabs: vec![tab], active: 0, tab_scroll: ScrollHandle::new() }
    }

    pub(crate) fn active(&self) -> usize {
        self.active
    }

    /// Set the active tab, clamped into range.
    pub(crate) fn set_active(&mut self, ix: usize) {
        self.active = ix.min(self.tabs.len().saturating_sub(1));
    }

    /// Re-clamp after the tab set shrank.
    pub(crate) fn clamp_active(&mut self) {
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
    }

    pub(crate) fn active_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub(crate) fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }
}
