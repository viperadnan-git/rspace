//! The file-list pane as a focusable child view (Zed `Pane`-style): owns the
//! directory listing, in-folder/recursive search, multi-selection, and sort.
//! Navigation, preview, context menus, and file operations stay on the
//! [`Workspace`]; the explorer reaches them through [`ExplorerEvent`] so a
//! callback never re-enters the explorer's own borrow.

use std::path::PathBuf;

use gpui::{EventEmitter, WeakEntity};

use super::*;

mod view;

/// Signals to the owning [`Workspace`]. Emitted from listeners/actions, handled
/// after the explorer update completes (so the workspace may call back in).
pub(crate) enum ExplorerEvent {
    /// Open a folder within the current remote.
    OpenDir(String),
    /// The cursor landed on a file — show it in the preview.
    OpenFile,
    /// Right-click on an entry: open its context menu at the cursor.
    Context(Entry, Point<Pixels>),
    /// Right-click on empty space: open the background menu at the cursor.
    Background(Point<Pixels>),
    /// External files dropped onto the list — upload into the open directory.
    Upload(Vec<PathBuf>),
    /// An entry dragged onto a folder (or the breadcrumb) — move/copy it.
    Drop { dragged: DraggedEntry, dst_remote: String, dst_dir: String, copy: bool },
    /// Sort field/order changed; the workspace persists it to settings.
    SortChanged(SortField, SortOrder),
}

pub(crate) struct Explorer {
    workspace: WeakEntity<Workspace>,
    service: Service,
    /// Folder-poll cadence, mirrored from settings via [`Self::set_refresh`].
    refresh_secs: u64,
    focus: FocusHandle,
    /// What the listing currently shows (pushed by the workspace on navigate).
    remote: Option<String>,
    path: String,
    sort_field: SortField,
    sort_order: SortOrder,
    dir_query: Query<(String, String), Vec<Entry>>,
    search_input: Entity<TextInput>,
    search_open: bool,
    search: String,
    /// The query whose recursive results `search_query` currently holds.
    searched: Option<String>,
    search_query: Query<(String, String, String), Vec<Entry>>,
    /// Displayed entries while a non-recursive filter is active, and the
    /// (query, dir-len) it was built for — so it's only rebuilt when those change.
    view: Vec<Entry>,
    view_sig: Option<(String, usize)>,
    /// The cursor / selection lead row, or `None` when nothing is selected
    /// (Finder-style: a fresh directory has no cursor). Invariant: `Some` iff
    /// `selected` is non-empty.
    entry_sel: Option<usize>,
    /// Multi-selection by entry path; survives re-sort and refresh.
    selected: HashSet<String>,
    sel_anchor: usize,
    entry_scroll: UniformListScrollHandle,
    /// A row to select by name once the next listing loads (e.g. the child
    /// folder after navigating up, or the renamed item).
    pending_select: Option<String>,
}

impl EventEmitter<ExplorerEvent> for Explorer {}

impl Focusable for Explorer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Explorer {
    pub(crate) fn new(
        workspace: WeakEntity<Workspace>,
        service: Service,
        sort_field: SortField,
        sort_order: SortOrder,
        refresh_secs: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let stale = Duration::from_secs(refresh_secs.max(1));
        let search_input = cx.new(|cx| TextInput::new(cx, "Search this folder").bare());
        // Only react to actual text changes — the input also notifies on caret
        // moves/selection, which don't affect the filter.
        cx.observe(&search_input, |this, input, cx| {
            let text = input.read(cx).text();
            if text != this.search {
                this.search = text.to_string();
                cx.notify();
            }
        })
        .detach();
        // Poll the open folder at the refresh cadence (focus-gated, self-cancelling).
        query::poll(
            window,
            cx,
            |e: &Self| Duration::from_secs(e.poll_secs()),
            Self::load_entries,
        );
        Self {
            workspace,
            service,
            refresh_secs,
            focus: cx.focus_handle(),
            remote: None,
            path: String::new(),
            sort_field,
            sort_order,
            dir_query: Query::new(Some(stale)),
            search_input,
            search_open: false,
            search: String::new(),
            searched: None,
            search_query: Query::new(None),
            view: Vec::new(),
            view_sig: None,
            entry_sel: None,
            selected: HashSet::new(),
            sel_anchor: 0,
            entry_scroll: UniformListScrollHandle::new(),
            pending_select: None,
        }
    }

    fn poll_secs(&self) -> u64 {
        self.refresh_secs.max(1)
    }

    /// Mirror the settings refresh cadence (folder poll + staleness window).
    pub(crate) fn set_refresh(&mut self, secs: u64) {
        self.refresh_secs = secs;
        self.dir_query.set_stale_after(Some(Duration::from_secs(secs.max(1))));
    }

    // --- listing --------------------------------------------------------------

    pub(crate) fn entries(&self) -> &[Entry] {
        if self.recursive_showing() {
            self.search_query.data().map(Vec::as_slice).unwrap_or(&[])
        } else if self.has_query() {
            &self.view
        } else {
            self.dir_query.data().map(Vec::as_slice).unwrap_or(&[])
        }
    }

    pub(crate) fn is_fetching(&self) -> bool {
        self.dir_query.is_fetching()
    }

    pub(crate) fn search_open(&self) -> bool {
        self.search_open
    }

    fn load_entries(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.remote.clone() else {
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

    pub(crate) fn force_reload_entries(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let (field, order) = (self.sort_field, self.sort_order);
        self.dir_query.reload(cx, |this| &mut this.dir_query, move |(remote, path)| async move {
            let mut entries = service.list_dir(&remote, &path).await?;
            sort_entries(&mut entries, field, order);
            Ok::<_, ServiceError>(entries)
        });
    }

    /// Show `remote:path`, resetting selection and search (Finder-style: a fresh
    /// directory has no selection unless `pending` names a row to land on).
    pub(crate) fn show(
        &mut self,
        remote: Option<String>,
        path: String,
        pending: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.reset_search(cx);
        self.remote = remote;
        self.path = path;
        self.entry_sel = None;
        self.sel_anchor = 0;
        self.selected.clear();
        self.pending_select = pending;
        if self.remote.is_some() {
            self.load_entries(cx);
        }
        cx.notify();
    }

    pub(crate) fn choose_sort(&mut self, field: SortField, cx: &mut Context<Self>) {
        if self.sort_field == field {
            self.sort_order = self.sort_order.toggle();
        } else {
            self.sort_field = field;
        }
        let (field, order) = (self.sort_field, self.sort_order);
        self.pending_select = self.cursor_name();
        self.dir_query.update_current(move |entries| sort_entries(entries, field, order));
        cx.emit(ExplorerEvent::SortChanged(field, order));
        cx.notify();
    }

    /// Apply a pending select-by-name once its listing has loaded, then clamp.
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
        // Drop selected paths that the new listing no longer contains, then keep
        // the cursor in range and consistent with the selection.
        if !self.selected.is_empty() {
            let valid: HashSet<String> = self.entries().iter().map(|e| e.path.clone()).collect();
            self.selected.retain(|p| valid.contains(p));
        }
        if self.selected.is_empty() {
            self.entry_sel = None;
        } else if let Some(ix) = self.entry_sel {
            self.entry_sel = Some(ix.min(self.entries().len().saturating_sub(1)));
        }
    }

    // --- search ---------------------------------------------------------------

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
        let Some(remote) = self.remote.clone() else {
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

    // --- selection ------------------------------------------------------------

    pub(crate) fn selected_entries(&self) -> Vec<Entry> {
        if self.selected.is_empty() {
            return Vec::new();
        }
        self.entries().iter().filter(|e| self.selected.contains(&e.path)).cloned().collect()
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

    pub(crate) fn is_selected(&self, path: &str) -> bool {
        self.selected.contains(path)
    }

    pub(crate) fn selection_len(&self) -> usize {
        self.selected.len()
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
        self.selected.clear();
        if let Some(e) = self.entries().get(ix) {
            self.selected.insert(e.path.clone());
            self.entry_sel = Some(ix);
            self.sel_anchor = ix;
        } else {
            self.entry_sel = None;
        }
    }

    pub(crate) fn toggle_at(&mut self, ix: usize) {
        if let Some(p) = self.entries().get(ix).map(|e| e.path.clone()) {
            if !self.selected.remove(&p) {
                self.selected.insert(p);
            }
        }
        self.sel_anchor = ix;
        self.entry_sel = (!self.selected.is_empty()).then_some(ix);
    }

    pub(crate) fn select_range_to(&mut self, ix: usize) {
        let (lo, hi) = (self.sel_anchor.min(ix), self.sel_anchor.max(ix));
        self.selected = self
            .entries()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= lo && *i <= hi)
            .map(|(_, e)| e.path.clone())
            .collect();
        self.entry_sel = (!self.selected.is_empty()).then_some(ix);
    }

    pub(crate) fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            self.selected.clear();
            self.entry_sel = None;
            cx.notify();
        }
    }

    /// On deliberate keyboard entry into the pane, land the cursor on the first
    /// row if nothing is selected — so the list is immediately navigable.
    pub(crate) fn select_first_if_empty(&mut self, cx: &mut Context<Self>) {
        if self.selected.is_empty() && !self.entries().is_empty() {
            self.select_only(0);
            self.scroll_to_selection();
            cx.notify();
        }
    }

    pub(crate) fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.selected = self.entries().iter().map(|e| e.path.clone()).collect();
        self.entry_sel = (!self.selected.is_empty()).then(|| self.entry_sel.unwrap_or(0));
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

    fn scroll_to_selection(&self) {
        if let Some(ix) = self.entry_sel {
            self.entry_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }
}
