//! The file-list pane as a focusable child view (Zed `Pane`-style): owns the
//! directory listing, in-folder/recursive search, multi-selection, and sort.
//! Navigation, preview, context menus, and file operations stay on the
//! [`Workspace`]; the explorer reaches them through [`ExplorerEvent`] so a
//! callback never re-enters the explorer's own borrow.

use std::path::PathBuf;

use gpui::{EventEmitter, WeakEntity};

use super::*;

actions!(explorer, [SearchSubmit, CloseSearch]);

/// Distance from the list's top/bottom edge (px) within which a marquee drag
/// auto-scrolls, and the per-frame scroll step.
const MARQUEE_EDGE: f32 = 24.0;
const MARQUEE_SCROLL_STEP: f32 = 12.0;

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
        let x = f32::from(e.event.position.x);
        let right = f32::from(e.bounds.right()) - TABLE_PAD;
        let date_w = f32::from(self.col_date_width);
        let (raw, current) = match e.drag(cx).0 {
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

    // --- search ---------------------------------------------------------------

    /// The shared search field, rendered by the navigator's locator slot.
    pub(crate) fn search_input(&self) -> Entity<TextInput> {
        self.search_input.clone()
    }

    pub(crate) fn search_is_empty(&self) -> bool {
        self.search.is_empty()
    }

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

    /// Toggle the search field. Opening leaves focusing the input to the action bar
    /// (the field lives in the [`ActionBar`], not here); closing returns focus to
    /// the list. Returns whether search is now open.
    pub(crate) fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.search_open = !self.search_open;
        if !self.search_open {
            self.reset_search(cx);
            self.focus.focus(window, cx);
        }
        cx.notify();
        self.search_open
    }

    pub(crate) fn close_search(&mut self, _: &CloseSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open {
            self.search_open = false;
            self.reset_search(cx);
            self.focus.focus(window, cx);
            cx.notify();
        }
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

    // --- marquee (rubber-band) selection --------------------------------------

    /// Begin (on first call) or continue a rubber-band drag from `anchor` to the
    /// live cursor `current`, both in window coords. `additive` (Cmd/Shift held at
    /// press) keeps the pre-drag selection; otherwise the band replaces it.
    pub(crate) fn drag_marquee(
        &mut self,
        anchor: Point<Pixels>,
        current: Point<Pixels>,
        additive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match &mut self.marquee {
            Some(m) => m.current = current,
            None => {
                if self.entries().is_empty() {
                    return;
                }
                let base = if additive { self.sel.snapshot().clone() } else { HashSet::new() };
                self.marquee = Some(Marquee { anchor, current, base });
                self.start_autoscroll(window, cx);
            }
        }
        self.apply_marquee();
        cx.notify();
    }

    pub(crate) fn end_marquee(&mut self, cx: &mut Context<Self>) {
        if self.marquee.take().is_some() {
            cx.notify();
        }
    }

    /// Recompute the selection from the band's current extent. Rebuilt from scratch
    /// each call (onto the pre-drag `base`), so shrinking the band deselects rows it
    /// no longer covers.
    fn apply_marquee(&mut self) {
        let Some(m) = self.marquee.as_ref() else {
            return;
        };
        let (anchor_y, cur_y) = (m.anchor.y, m.current.y);
        let mut selected = m.base.clone();
        let mut lead = None;
        if let Some((lo, hi)) = self.marquee_rows(anchor_y, cur_y) {
            for ix in lo..=hi {
                if let Some(e) = self.entries().get(ix) {
                    selected.insert(e.path.clone());
                }
            }
            lead = Some(if cur_y >= anchor_y { hi } else { lo });
        }
        self.entry_sel = lead.filter(|_| !selected.is_empty());
        self.sel.set_to(selected);
    }

    /// Edge-scroll loop for a marquee drag; self-terminates once the band ends
    /// (autoscroll_tick returns false), since gpui only fires drag-move on motion.
    fn start_autoscroll(&self, window: &Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(16)).await;
                let alive = cx
                    .update(|_, app| this.update(app, |this, cx| this.autoscroll_tick(cx)))
                    .map(|r| r.unwrap_or(false));
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One auto-scroll frame: nudge the list when the cursor sits in an edge zone,
    /// then re-derive the selection. Returns whether the band is still active.
    fn autoscroll_tick(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(cur_y) = self.marquee.as_ref().map(|m| f32::from(m.current.y)) else {
            return false;
        };
        let st = self.entry_scroll.0.borrow();
        let bounds = st.base_handle.bounds();
        let (top, height) = (f32::from(bounds.top()), f32::from(bounds.size.height));
        let off = st.base_handle.offset();
        let (off_y, max_y) = (f32::from(off.y), f32::from(st.base_handle.max_offset().y));
        let step = if cur_y < top + MARQUEE_EDGE && off_y < 0.0 {
            MARQUEE_SCROLL_STEP
        } else if cur_y > top + height - MARQUEE_EDGE && off_y > -max_y {
            -MARQUEE_SCROLL_STEP
        } else {
            return true;
        };
        let new_y = (off_y + step).clamp(-max_y, 0.0);
        if (new_y - off_y).abs() < 0.5 {
            return true;
        }
        st.base_handle.set_offset(Point { x: off.x, y: px(new_y) });
        drop(st);
        self.apply_marquee();
        cx.notify();
        true
    }

    /// Row indices whose vertical extent intersects the band between window
    /// y-coords `y0` and `y1`. `None` when the list is empty, not yet laid out, or
    /// the band misses the rows entirely.
    fn marquee_rows(&self, y0: Pixels, y1: Pixels) -> Option<(usize, usize)> {
        let st = self.entry_scroll.0.borrow();
        let len = self.entries().len();
        if len == 0 {
            return None;
        }
        // `last_item_size.item` is the viewport, `.contents` the full content stack;
        // a single row is the content height over the row count.
        let row_h = f32::from(st.last_item_size?.contents.height) / len as f32;
        if row_h <= 0.0 {
            return None;
        }
        let top = f32::from(st.base_handle.bounds().top() + st.base_handle.offset().y);
        let bottom = top + row_h * len as f32;
        let (lo, hi) = (f32::from(y0).min(f32::from(y1)), f32::from(y0).max(f32::from(y1)));
        if hi < top || lo > bottom {
            return None;
        }
        let first = ((lo - top) / row_h).floor().max(0.0) as usize;
        let last = (((hi - top) / row_h).floor() as usize).min(len - 1);
        Some((first.min(last), last))
    }

    /// The band rectangle to paint as `(left, top, width, height)` relative to the
    /// list viewport's top-left, clamped to the viewport. `None` when no drag is
    /// active or the list hasn't been laid out.
    pub(crate) fn marquee_rect(&self) -> Option<(Pixels, Pixels, Pixels, Pixels)> {
        let m = self.marquee.as_ref()?;
        let st = self.entry_scroll.0.borrow();
        st.last_item_size?;
        let vp = st.base_handle.bounds();
        let (ox, oy) = (f32::from(vp.left()), f32::from(vp.top()));
        let (w, h) = (f32::from(vp.size.width), f32::from(vp.size.height));
        let cx0 = (f32::from(m.anchor.x) - ox).clamp(0.0, w);
        let cx1 = (f32::from(m.current.x) - ox).clamp(0.0, w);
        let cy0 = (f32::from(m.anchor.y) - oy).clamp(0.0, h);
        let cy1 = (f32::from(m.current.y) - oy).clamp(0.0, h);
        Some((px(cx0.min(cx1)), px(cy0.min(cy1)), px((cx1 - cx0).abs()), px((cy1 - cy0).abs())))
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

    fn scroll_to_selection(&self) {
        if let Some(ix) = self.entry_sel {
            self.entry_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }
}
