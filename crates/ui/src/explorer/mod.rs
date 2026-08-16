//! The file-list pane as a focusable child view (Zed `Pane`-style): owns the
//! directory listing, in-folder/recursive search, multi-selection, and sort.
//! Navigation, preview, context menus, and file operations stay on the
//! [`Workspace`]; the explorer reaches them through [`ExplorerEvent`] so a
//! callback never re-enters the explorer's own borrow.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use gpui::{EventEmitter, WeakEntity};

use super::*;

/// Boxed listing future: names the fetcher's return so one `dir_fetch` builder
/// can feed `dir_query`'s load/reload/invalidate.
type DirFetch = Pin<Box<dyn Future<Output = Result<Vec<Entry>, ServiceError>>>>;

actions!(explorer, [SearchSubmit, CloseSearch]);

mod marquee;
mod search;
mod selection;
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
    /// External files dropped onto the list — upload into the dropped-on pane's
    /// directory. Carries its own destination (like [`Self::Drop`]) because the
    /// workspace can't infer it: a file drop never moves focus.
    Upload { paths: Vec<PathBuf>, dst_remote: String, dst_dir: String },
    /// An entry dragged onto a folder (or the breadcrumb) — move/copy it.
    Drop { dragged: DraggedEntry, dst_remote: String, dst_dir: String, mods: Modifiers },
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
    /// (Finder-style: a fresh directory has no cursor). Invariant: `Some` iff `sel`
    /// is non-empty. The cursor (keyboard nav, preview/rename subject) is distinct
    /// from the selection set, so it stays here rather than in `sel`.
    entry_sel: Option<usize>,
    /// Multi-selection by entry path (survives re-sort and refresh) + range anchor.
    sel: Selection<String>,
    /// Active rubber-band selection (press-drag in empty list space): the press
    /// point and live cursor in window coords, plus the selection that predated
    /// the drag (kept when additive, empty otherwise).
    marquee: Option<Marquee>,
    /// Window-coord of the last left-press in empty list space — the band anchor.
    marquee_anchor: Point<Pixels>,
    entry_scroll: UniformListScrollHandle,
    /// A row to select by name once the next listing loads (e.g. the child
    /// folder after navigating up, or the renamed item).
    pending_select: Option<String>,
    /// Size / Date column widths (resizable; persisted by the workspace).
    col_date_width: Pixels,
    col_size_width: Pixels,
    /// Whether this explorer belongs to the active tab. Background tabs skip the
    /// folder poll so N open tabs don't fan out into N periodic RC listings.
    is_active: bool,
    /// Compare overlay (full path → state) from the last sync compare; rows render
    /// a tint for their state. `diff_dirs` holds each ancestor dir's aggregated
    /// state (all descendants same → that state, else `Differ`). Empty when no
    /// compare is shown.
    diff: HashMap<String, DiffState>,
    diff_dirs: HashMap<String, DiffState>,
}

/// In-flight rubber-band selection state. See [`Explorer::marquee`].
struct Marquee {
    anchor: Point<Pixels>,
    current: Point<Pixels>,
    base: HashSet<String>,
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
        sort: (SortField, SortOrder),
        refresh_secs: u64,
        cols: (Pixels, Pixels),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (sort_field, sort_order) = sort;
        let (col_date_width, col_size_width) = cols;
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
        // Poll the open folder at the refresh cadence (window-active- and
        // active-tab-gated, self-cancelling).
        query::poll(
            window,
            cx,
            |e: &Self| Duration::from_secs(e.poll_secs()),
            Self::poll_tick,
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
            sel: Selection::new(),
            marquee: None,
            marquee_anchor: Point::default(),
            entry_scroll: UniformListScrollHandle::new(),
            pending_select: None,
            col_date_width,
            col_size_width,
            is_active: false,
            diff: HashMap::new(),
            diff_dirs: HashMap::new(),
        }
    }

    /// Overlay the compare result (paths relative to `root`) onto the listing.
    /// Matches are dropped; ancestor dirs are flagged so a folder row aggregates.
    pub(crate) fn set_diff(&mut self, entries: &[DiffEntry], root: &str, cx: &mut Context<Self>) {
        self.diff.clear();
        self.diff_dirs.clear();
        for e in entries {
            let full = if root.is_empty() { e.path.clone() } else { format!("{root}/{}", e.path) };
            // Only differing files get a row tint; matches don't.
            if e.state != DiffState::Match {
                self.diff.insert(full.clone(), e.state);
            }
            // But fold *every* descendant — matches included — into ancestor dirs,
            // so a folder that exists on both (has matches) reads as "differs" once
            // it also holds new content, rather than "entirely new".
            let mut p = full.as_str();
            while let Some((parent, _)) = p.rsplit_once('/') {
                self.diff_dirs
                    .entry(parent.to_string())
                    .and_modify(|s| {
                        if *s != e.state {
                            *s = DiffState::Differ;
                        }
                    })
                    .or_insert(e.state);
                p = parent;
            }
        }
        cx.notify();
    }

    pub(crate) fn clear_diff(&mut self, cx: &mut Context<Self>) {
        if !self.diff.is_empty() || !self.diff_dirs.is_empty() {
            self.diff.clear();
            self.diff_dirs.clear();
            cx.notify();
        }
    }

    /// The compare marker for a row: its own state, or `Differ` for a folder with a
    /// differing descendant.
    pub(crate) fn entry_diff(&self, entry: &Entry) -> Option<DiffState> {
        if let Some(state) = self.diff.get(&entry.path) {
            return Some(*state);
        }
        if !entry.is_dir {
            return None;
        }
        // A folder of only matches is identical — no tint.
        match self.diff_dirs.get(&entry.path).copied() {
            Some(DiffState::Match) | None => None,
            other => other,
        }
    }

    fn poll_secs(&self) -> u64 {
        self.refresh_secs.max(1)
    }

    /// Periodic tick: only the active tab's explorer polls its folder.
    fn poll_tick(&mut self, cx: &mut Context<Self>) {
        if self.is_active {
            self.load_entries(cx);
        }
    }

    /// Mark this explorer active/inactive (workspace drives it on tab switch).
    /// Becoming active triggers a stale-aware refresh so a returned-to tab shows
    /// current contents without a blocking refetch when already fresh.
    pub(crate) fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.is_active == active {
            return;
        }
        self.is_active = active;
        if active && self.remote.is_some() {
            self.load_entries(cx);
        }
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

    /// The open `(remote, path)`, or `None` on the welcome screen.
    pub(crate) fn location(&self) -> Option<(String, String)> {
        self.remote.clone().map(|r| (r, self.path.clone()))
    }

    pub(crate) fn col_date_width(&self) -> Pixels {
        self.col_date_width
    }

    pub(crate) fn col_size_width(&self) -> Pixels {
        self.col_size_width
    }

    /// Resize a column by dragging its left divider. Widths are measured from the
    /// table's right content edge (the Name column flex-grows to fill).
    pub(crate) fn on_column_drag(&mut self, e: &DragMoveEvent<DragColumn>, _: &mut Window, cx: &mut Context<Self>) {
        let (owner, col) = {
            let drag = e.drag(cx);
            (drag.owner, drag.col)
        };
        if owner != cx.entity_id() {
            return;
        }
        let x = f32::from(e.event.position.x);
        let right = f32::from(e.bounds.right()) - TABLE_PAD;
        let date_w = f32::from(self.col_date_width);
        let (raw, current) = match col {
            Column::Date => (right - x, &mut self.col_date_width),
            Column::Size => (right - date_w - x, &mut self.col_size_width),
        };
        let width = px(raw.clamp(COL_MIN, COL_MAX));
        if width != *current {
            *current = width;
            cx.notify();
        }
    }

    pub(crate) fn reset_column(&mut self, column: Column, cx: &mut Context<Self>) {
        match column {
            Column::Date => self.col_date_width = px(COL_DATE),
            Column::Size => self.col_size_width = px(COL_SIZE),
        }
        cx.notify();
    }

    /// The one directory fetcher (list + sort under the current sort), captured
    /// fresh per call. All three `dir_query` entry points route through it.
    fn dir_fetch(&self) -> impl FnOnce((String, String)) -> DirFetch + use<> {
        let service = self.service.clone();
        let (field, order) = (self.sort_field, self.sort_order);
        move |(remote, path)| {
            Box::pin(async move {
                let mut entries = service.list_dir(&remote, &path).await?;
                sort_entries(&mut entries, field, order);
                Ok(entries)
            })
        }
    }

    fn load_entries(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.remote.clone() else {
            return;
        };
        let key = (remote, self.path.clone());
        let fetch = self.dir_fetch();
        self.dir_query.load(key, cx, |this| &mut this.dir_query, fetch);
    }

    pub(crate) fn force_reload_entries(&mut self, cx: &mut Context<Self>) {
        let fetch = self.dir_fetch();
        self.dir_query.reload(cx, |this| &mut this.dir_query, fetch);
    }

    /// Invalidate the cached listing for `remote:dir`; if this explorer is showing
    /// it, refetch now (otherwise the next visit refetches). Driven by job
    /// completion — the dir a job touched may be open here or in another pane.
    pub(crate) fn invalidate_dir(&mut self, remote: &str, dir: &str, cx: &mut Context<Self>) {
        let key = (remote.to_string(), dir.to_string());
        let fetch = self.dir_fetch();
        self.dir_query.invalidate(&key, cx, |this| &mut this.dir_query, fetch);
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
        self.sel.clear();
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
        if !self.sel.is_empty() {
            let valid: HashSet<String> = self.entries().iter().map(|e| e.path.clone()).collect();
            self.sel.retain(|p| valid.contains(p));
        }
        if self.sel.is_empty() {
            self.entry_sel = None;
        } else if let Some(ix) = self.entry_sel {
            self.entry_sel = Some(ix.min(self.entries().len().saturating_sub(1)));
        }
    }
}
