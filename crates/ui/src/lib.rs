//! gpui desktop shell: a two-pane remote browser.

mod command_palette;
mod components;
mod fuzzy;
mod jobs;
mod menus;
mod mount_options;
mod panels;
mod preview;
mod query;
mod remotes;
mod theme;
mod transfers;
mod views;
mod bootstrap;
mod status_screen;
mod workspace;

// Re-export the reusable components at the crate root so existing `use <c>::…`
// and `crate::<c>::…` paths resolve unchanged after the move under `components`.
pub(crate) use components::{confirm, number_field, picker, prompt, text_input, toast, widgets};

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::time::Duration;

use gpui::{
    actions, anchored, deferred, div, point, prelude::*, px, relative, rgb, rgba, size, svg,
    uniform_list, AnyElement, App, AssetSource, Bounds, ClickEvent, ClipboardItem, Context,
    DismissEvent, Div, DragMoveEvent, Entity, ExternalPaths, FocusHandle, Focusable, KeyBinding,
    Menu, MenuItem,
    MouseButton, MouseDownEvent, MouseUpEvent,
    PathPromptOptions, Pixels, Point, ScrollStrategy, SharedString, Stateful, TitlebarOptions,
    UniformListScrollHandle, Window, WindowBounds, WindowOptions,
};
use gpui_platform::application;
use rspace_core::{
    dir_size, mount_root, Db, JobRecord, Paths, SettingsStore, SortField, SortOrder, UiState,
};
use rspace_rclone_rc::{
    ArgKind, ArgSpec, ArgValue, ConfigPaths, Entry, InfoOp, InfoResult, Matcher, MountConfig,
    Operation, Provider, RemoteInfo, RemoteOption, Service, ServiceError, TransferMode,
};

use preview::{Preview, PreviewState};
use command_palette::CommandPaletteDelegate;
use confirm::ConfirmModal;
use picker::Picker;
use prompt::PromptModal;
use toast::{ToastBody, Toasts};
use workspace::modal::ActiveModal;
use transfers::{Job, JobTarget, Jobs, JobsEvent};
use number_field::{NumberField, NumberFieldEvent};
use text_input::TextInput;
use query::{Query, Status};
use theme::*;
use widgets::*;

const JOB_HISTORY_LIMIT: usize = 50;
/// Recent remotes fetched into the cache; the welcome screen filters these
/// against the live config and shows the first few, so over-fetch to survive
/// remotes that were since deleted.
const RECENT_REMOTES_FETCH: usize = 20;
const RECENT_REMOTES_SHOWN: usize = 5;

actions!(
    rspace,
    [
        Quit,
        SelectNext,
        SelectPrev,
        Open,
        GoUp,
        GoBack,
        GoForward,
        Reload,
        TogglePane,
        FocusSidebar,
        FocusExplorer,
        Minimize,
        Zoom,
        ToggleFullscreen,
        CloseSettings,
        CopyEntry,
        CutEntry,
        PasteEntry,
        DeleteEntry,
        ConfirmAccept,
        SelectAll,
        NewFolder,
        NewFile,
        Rename,
        TogglePreview,
        ConfigNext,
        ConfigPrev,
        ConfigConfirm,
        FocusNext,
        FocusPrev,
        PromptSubmit,
        PromptCancel,
        TogglePalette,
        AddRemote,
        OpenSettings,
        RestartDaemon,
        ToggleTransfers,
        MountSave,
        MountCancel,
        SetupSubmit,
        NumberCommit,
        SearchSubmit,
        ToggleSearch,
        CloseSearch
    ]
);

pub use bootstrap::{run, RcloneStatus, Startup};
pub(crate) use status_screen::{relaunch, StatusScreen};

#[derive(PartialEq, Clone, Copy)]
enum Pane {
    Sidebar,
    Explorer,
}

#[derive(PartialEq, Clone, Copy)]
enum CopySource {
    Path,
    Error,
    JobCommand(usize),
}

#[derive(Clone, Copy, PartialEq)]
enum RcloneField {
    Binary,
    Config,
}

impl RcloneField {
    fn placeholder(self) -> &'static str {
        match self {
            RcloneField::Binary => "Path to the rclone binary",
            RcloneField::Config => "Path to the rclone config file",
        }
    }
}

/// Reachability of the rclone rc daemon, surfaced by the status-bar button.
#[derive(Clone)]
enum RcHealth {
    Unknown,
    Up,
    Down(String),
    /// Daemon is being restarted (a fresh `rcd` is spawning).
    Restarting,
}

impl RcHealth {
    fn icon(&self) -> &'static str {
        match self {
            RcHealth::Down(_) => "icons/server_network_off.svg",
            _ => "icons/server_network.svg",
        }
    }
}

#[derive(Clone)]
struct Location {
    remote: String,
    path: String,
    selected: Option<String>,
}

/// Source for a cross-remote copy/cut, resolved against the destination at paste.
#[derive(Clone)]
struct Clipboard {
    remote: String,
    entries: Vec<Entry>,
    mode: TransferMode,
}

#[derive(Clone, Copy, PartialEq)]
enum ResizeTarget {
    Sidebar,
    Preview,
}

#[derive(Clone)]
struct DragResize(ResizeTarget);

impl Render for DragResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Column {
    Date,
    Size,
}

#[derive(Clone)]
struct DragColumn(Column);

impl Render for DragColumn {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

struct DraggedRemote {
    name: String,
}

/// An explorer entry being dragged onto a folder. `count` lets the preview read
/// "N items" when the dragged row is part of the multi-selection.
struct DraggedEntry {
    path: String,
    name: String,
    is_dir: bool,
    count: usize,
}

struct DragLabel {
    text: SharedString,
}

impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(ELEVATED))
            .shadow_lg()
            .text_xs()
            .text_color(rgb(FG))
            .child(self.text.clone())
    }
}

struct Workspace {
    service: Service,
    rclone_bin: String,
    version: String,
    focus: FocusHandle,
    pane: Pane,
    remotes: Vec<RemoteInfo>,
    remote_sel: usize,
    remote_scroll: UniformListScrollHandle,
    /// Right-click menu on a remote: the remote name and the cursor position.
    remote_menu: Option<(String, Point<Pixels>)>,
    /// In-progress rclone binary/config override edit in Settings (field + input).
    rclone_edit: Option<(RcloneField, Entity<TextInput>)>,
    /// Focus the rclone edit input once when it opens (not every frame, which
    /// would trap focus in it).
    rclone_edit_focus: bool,
    /// Per-remote mount config (cache mode, read-only, limits); cached from the
    /// DB, edited via the mount-options modal, read when mounting.
    mount_configs: HashMap<String, MountConfig>,
    sidebar_width: Pixels,
    preview_width: Pixels,
    col_date_width: Pixels,
    col_size_width: Pixels,
    open_remote: Option<String>,
    /// Empty = root.
    path: String,
    entry_sel: usize,
    /// Multi-selection by entry path; survives re-sort and refresh. Always
    /// contains the cursor's path unless explicitly toggled off.
    selected: HashSet<String>,
    sel_anchor: usize,
    entry_scroll: UniformListScrollHandle,
    /// A row to select by name once the next listing loads (e.g. the child
    /// folder after navigating up).
    pending_select: Option<String>,
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
    history: Vec<Location>,
    history_pos: usize,
    /// Last folder viewed per remote; reopening a remote returns to it.
    remote_paths: HashMap<String, String>,
    copied: Option<CopySource>,
    sort_field: SortField,
    sort_order: SortOrder,
    paths: Paths,
    store: SettingsStore,
    db: Db,
    /// Cached layout state, mirrored to `db` on change via [`Self::save_ui`].
    ui: UiState,
    pinned: Vec<String>,
    /// Recently-opened remote names (newest first); refreshed on navigate, read
    /// by the welcome screen — kept out of the render path.
    recent_remotes: Vec<String>,
    job_history: Vec<JobRecord>,
    /// Names of mounted remotes; refreshed after a mount/unmount, read by the
    /// sidebar and remote menu — kept out of the render path.
    mounted: HashSet<String>,
    storage_size: Option<(u64, u64)>,
    rclone_paths: Option<ConfigPaths>,
    rclone_cache_size: Option<u64>,
    settings_open: bool,
    context: Option<(Entry, Point<Pixels>)>,
    bg_menu: Option<Point<Pixels>>,
    rc_popover_open: bool,
    /// The single active modal overlay (palette, remote config, mount options,
    /// confirm). At most one is open; see [`ActiveModal`].
    modal: Option<ActiveModal>,
    prompt: Option<Entity<PromptModal>>,
    prompt_sub: Option<gpui::Subscription>,
    toasts: Entity<Toasts>,
    jobs: Entity<Jobs>,
    refresh_field: Entity<NumberField>,
    jobs_open: bool,
    jobs_maximized: bool,
    preview_open: bool,
    preview: Option<Preview>,
    /// Recently loaded previews, keyed by `remote:path` (LRU, bounded).
    preview_cache: Vec<(String, PreviewState)>,
    rc_health: RcHealth,
    clipboard: Option<Clipboard>,
}

impl Workspace {
    fn new(
        service: Service,
        rclone_bin: String,
        version: String,
        paths: Paths,
        store: SettingsStore,
        db: Db,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let stale = Duration::from_secs(store.get().refresh_secs.max(1));
        let (sort_field, sort_order) = (store.get().sort_field, store.get().sort_order);
        let ui = db.load_ui();
        let pinned = db.load_pinned();
        let recent_remotes = db.recent_remotes(RECENT_REMOTES_FETCH);
        let job_history = db.recent_jobs(JOB_HISTORY_LIMIT);
        let mount_configs = db
            .load_mount_configs()
            .into_iter()
            .filter_map(|(name, json)| serde_json::from_str(&json).ok().map(|c| (name, c)))
            .collect();
        let sidebar_width = clamped_width(ui.sidebar_width, SIDEBAR_W, SIDEBAR_MIN, SIDEBAR_MAX);
        let preview_width = clamped_width(ui.preview_width, PREVIEW_W, PREVIEW_MIN, PREVIEW_MAX);
        let col_date_width = clamped_width(ui.col_date_width, COL_DATE, COL_MIN, COL_MAX);
        let col_size_width = clamped_width(ui.col_size_width, COL_SIZE, COL_MIN, COL_MAX);
        let jobs_maximized = ui.transfers_maximized;
        let preview_open = ui.preview_open;
        let jobs = cx.new(|_| Jobs::new(service.clone(), db.clone()));
        jobs.update(cx, |jobs, cx| jobs.start_polling(cx));
        let refresh_field = cx.new(|cx| NumberField::new(store.get().refresh_secs, 1, 120, 1, cx));
        cx.subscribe(&refresh_field, |this, _, ev, cx| {
            let NumberFieldEvent::Changed(secs) = ev;
            this.set_refresh(*secs, cx);
        })
        .detach();
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
        let toasts = cx.new(|cx| Toasts::new(window, cx));
        cx.subscribe(&jobs, |this, _, event, cx| match event {
            JobsEvent::ReloadEntries => this.force_reload_entries(cx),
            JobsEvent::Finished { label, ok, error } => {
                this.job_history = this.db.recent_jobs(JOB_HISTORY_LIMIT);
                if *ok {
                    this.toast(label.clone(), false, cx);
                } else {
                    // Surface rclone's own reason, not just "failed".
                    let msg = match error {
                        Some(e) => format!("{label} failed \u{2014} {e}"),
                        None => format!("{label} failed"),
                    };
                    this.toast_sticky(msg, true, cx);
                }
                cx.notify();
            }
        })
        .detach();
        let this = Self {
            service,
            rclone_bin,
            version,
            focus,
            pane: Pane::Sidebar,
            remotes: Vec::new(),
            remote_sel: 0,
            remote_scroll: UniformListScrollHandle::new(),
            remote_menu: None,
            rclone_edit: None,
            rclone_edit_focus: false,
            mount_configs,
            sidebar_width,
            preview_width,
            col_date_width,
            col_size_width,
            open_remote: None,
            path: String::new(),
            entry_sel: 0,
            selected: HashSet::new(),
            sel_anchor: 0,
            entry_scroll: UniformListScrollHandle::new(),
            pending_select: None,
            dir_query: Query::new(Some(stale)),
            search_input,
            search_open: false,
            search: String::new(),
            searched: None,
            search_query: Query::new(None),
            view: Vec::new(),
            view_sig: None,
            history: Vec::new(),
            history_pos: 0,
            remote_paths: HashMap::new(),
            copied: None,
            sort_field,
            sort_order,
            paths,
            store,
            db,
            ui,
            pinned,
            recent_remotes,
            job_history,
            mounted: HashSet::new(),
            storage_size: None,
            rclone_paths: None,
            rclone_cache_size: None,
            settings_open: false,
            context: None,
            bg_menu: None,
            rc_popover_open: false,
            modal: None,
            prompt: None,
            prompt_sub: None,
            toasts,
            jobs,
            refresh_field,
            jobs_open: false,
            jobs_maximized,
            preview_open,
            preview: None,
            preview_cache: Vec::new(),
            rc_health: RcHealth::Unknown,
            clipboard: None,
        };
        this.load_remotes(cx);
        Self::poll_health(window, cx);
        // Poll the open folder at the refresh cadence (focus-gated, self-cancelling).
        query::poll(
            window,
            cx,
            |v: &Self| Duration::from_secs(v.store.get().refresh_secs.max(1)),
            Self::load_entries,
        );
        this
    }

    /// Ping the rc daemon on an interval for the status-bar dot; runs unfocused.
    fn poll_health(window: &Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let service = match cx.update(|_, app| this.update(app, |v, _| v.service.clone()).ok()) {
                    Ok(Some(s)) => s,
                    _ => break,
                };
                let health = match service.ping().await {
                    Ok(()) => RcHealth::Up,
                    Err(e) => RcHealth::Down(e.to_string()),
                };
                let alive = cx
                    .update(|_, app| {
                        this.update(app, |v, vcx| {
                            // Don't fight an in-flight restart (it pings the old, dead port).
                            if !matches!(v.rc_health, RcHealth::Restarting) {
                                v.rc_health = health;
                                vcx.notify();
                            }
                        })
                        .is_ok()
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(3)).await;
            }
        })
        .detach();
    }
}
