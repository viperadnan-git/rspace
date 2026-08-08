//! gpui desktop shell: a two-pane remote browser.

mod command_palette;
mod components;
mod daemon;
mod drag;
mod explorer;
mod fuzzy;
mod jobs;
mod keybindings;
mod keymap;
mod menus;
mod mount_options;
mod panels;
mod action_bar;
mod preview;
mod query;
mod selection;
mod sync_pane;
mod tasks_pane;
mod remotes;
mod sidebar;
mod spring;
mod theme;
mod transfers;
mod update;
mod views;
mod bootstrap;
mod status_screen;
mod workspace;

// Re-export the reusable components at the crate root so existing `use <c>::…`
// and `crate::<c>::…` paths resolve unchanged after the move under `components`.
pub(crate) use components::{confirm, number_field, picker, prompt, text_input, toast, widgets};
pub(crate) use drag::*;

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::time::Duration;

use gpui::{
    actions, anchored, deferred, div, point, prelude::*, px, relative, rgb, rgba, size, svg,
    uniform_list, AnyElement, App, AssetSource, Bounds, ClickEvent, ClipboardItem, Context,
    DismissEvent, Div, DragMoveEvent, Entity, FocusHandle, Focusable,
    Menu, MenuItem, Modifiers,
    MouseButton, MouseDownEvent, MouseUpEvent,
    PathPromptOptions, Pixels, Point, ScrollHandle, ScrollStrategy, SharedString, Stateful,
    TitlebarOptions, UniformListScrollHandle, WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_platform::application;
use rspace_core::{
    dir_size, mount_root, Db, Paths, SettingsStore, SortField, SortOrder, UiState,
};
use rspace_rclone_rc::{
    ArgKind, ArgSpec, ArgValue, ConfigPaths, DiffEntry, DiffState, Entry, InfoOp, InfoResult,
    Matcher, MountConfig, Operation, Provider, RemoteInfo, RemoteOption, Service, ServiceError,
    SyncMode, TransferMode,
};

use preview::PreviewPane;
use selection::Selection;
use tasks_pane::TasksPane;
use sync_pane::SyncPane;
use action_bar::ActionBar;
use spring::SpringLoad;
use command_palette::CommandPaletteDelegate;
use confirm::ConfirmModal;
use keybindings::KeybindingsView;
use picker::Picker;
use prompt::PromptModal;
use toast::{ToastBody, Toasts};
use workspace::modal::ActiveModal;
use workspace::pane::{Pane, PaneGroup};
use transfers::{Job, JobTarget, Jobs, JobsEvent};
use number_field::{NumberField, NumberFieldEvent};
use explorer::{Explorer, ExplorerEvent};
use sidebar::{Sidebar, SidebarEvent};
use daemon::{DaemonStatus, RcHealth};
use text_input::TextInput;
use query::{Query, Status};
use theme::*;
use widgets::*;

/// Recent remotes fetched into the cache; the welcome screen filters these
/// against the live config and shows the first few, so over-fetch to survive
/// remotes that were since deleted.
const RECENT_REMOTES_FETCH: usize = 20;
const RECENT_REMOTES_SHOWN: usize = 5;

/// App version. CI is the single source of truth: it sets `RSPACE_VERSION` (from
/// the release tag) at build time. Local/dev builds fall back to the crate version.
pub const VERSION: &str = match option_env!("RSPACE_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

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
        ToggleSplit,
        TogglePalette,
        AddRemote,
        OpenSettings,
        RestartDaemon,
        CheckForUpdates,
        Uninstall,
        ToggleTasks,
        ToggleSync,
        ToggleSearch,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        ShowKeybindings,
        NewTab,
        CloseTab,
        NextTab,
        PrevTab,
        ActivateTab1,
        ActivateTab2,
        ActivateTab3,
        ActivateTab4,
        ActivateTab5,
        ActivateTab6,
        ActivateTab7,
        ActivateTab8,
        ActivateTab9
    ]
);

pub use bootstrap::{run, RcloneStatus, Startup};
pub(crate) use status_screen::{relaunch, StatusScreen};

#[derive(PartialEq, Clone, Copy)]
enum CopySource {
    Error,
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

#[derive(Clone)]
struct Location {
    remote: String,
    path: String,
    selected: Option<String>,
}

/// Transient state of the settings panel — never the source of truth. Persisted
/// preferences live in [`Workspace::store`] (`SettingsStore` → disk); this struct
/// holds only the panel's working copies and read-only fetches:
///   - **input widgets** (`rclone_edit`, `refresh_field`, `ui_font_field`) are
///     edit buffers seeded from `store` and written back to it on commit — they
///     mirror `store`, they don't own the value;
///   - **fetched displays** (`storage_size`, `rclone_paths`, `rclone_cache_size`,
///     `rclone_bin`) are read-only info pulled from the daemon/env for display,
///     not settings at all.
/// Rendered by `panels::settings`, orchestrated by `workspace::settings`.
struct SettingsView {
    open: bool,
    /// The rclone binary path in use (fetched, shown in the rclone setting).
    rclone_bin: String,
    /// In-progress rclone binary/config override edit (field + input).
    rclone_edit: Option<(RcloneField, Entity<TextInput>)>,
    /// Focus the rclone edit input once when it opens (not every frame, which
    /// would trap focus in it).
    rclone_edit_focus: bool,
    /// Edit buffer for `store`'s refresh interval; committed on change.
    refresh_field: Entity<NumberField>,
    /// Edit buffer for `store`'s `ui_font_size` (px); committed on change.
    ui_font_field: Entity<NumberField>,
    storage_size: Option<(u64, u64)>,
    rclone_paths: Option<ConfigPaths>,
    rclone_cache_size: Option<u64>,
}

/// Transient overlay menus, reset as a unit by [`Workspace::close_menus`].
#[derive(Default)]
struct Menus {
    /// Entry context menu: the entry and the cursor position.
    context: Option<(Entry, Point<Pixels>)>,
    /// Background (empty-space) menu position.
    bg_menu: Option<Point<Pixels>>,
    /// Remote right-click menu: the remote name and the cursor position.
    remote_menu: Option<(String, Point<Pixels>)>,
    /// Tab right-click menu: the tab's id and the cursor position.
    tab_menu: Option<(usize, Point<Pixels>)>,
    /// Task-row right-click menu: the selected job ids it acts on and the cursor
    /// position. One id → the full single-row menu; many → bulk actions.
    task_menu: Option<(Vec<usize>, Point<Pixels>)>,
    /// The rc-daemon health popover (status bar).
    rc_popover_open: bool,
    /// The sync compare/sync popover (status bar).
    sync_popover_open: bool,
}

/// Source for a cross-remote copy/cut, resolved against the destination at paste.
#[derive(Clone)]
struct Clipboard {
    remote: String,
    entries: Vec<Entry>,
    mode: TransferMode,
}

/// The app's shared, cheap-to-clone dependencies, bundled so the workspace holds
/// one field instead of three; cloned out to async tasks as needed.
#[derive(Clone)]
struct AppState {
    service: Service,
    db: Db,
    paths: Paths,
}

/// One browse context — the unit a tab owns: its [`Pane`] (explorer + action bar
/// + the location/history it's browsing). Lives in a [`PaneGroup`]; the workspace
/// renders the active tab of each group.
struct Tab {
    /// Stable identity (survives reordering on pin/unpin); used to track the
    /// active tab and to target context-menu actions.
    id: usize,
    /// Session-only pin. Pinned tabs sort before unpinned, render compact with no
    /// close button, and close only via the context menu.
    pinned: bool,
    pane: Pane,
}

/// A panel that can occupy the right dock. At most one shows at a time; toggling
/// one replaces the other. Extensible: add a variant, give it `title`/`icon` and
/// a body arm in `dock.rs`, and a toggle. The panel's own state lives in its own
/// entity (`PreviewPane`, `Jobs`), never duplicated on the workspace.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Panel {
    Preview,
    Tasks,
}

struct Workspace {
    app: AppState,
    /// The running rclone binary's version (shown in the status bar).
    version: String,
    focus: FocusHandle,
    remotes: Vec<RemoteInfo>,
    /// The remotes sidebar pane (owns the cursor/scroll/focus).
    sidebar: Entity<Sidebar>,
    _sidebar_sub: gpui::Subscription,
    /// Transient right-click menus + the daemon popover (all reset together by
    /// [`Self::close_menus`]).
    menus: Menus,
    /// The settings panel and its local state (visibility, rclone-override edit,
    /// fetched storage/cache info, refresh-interval field).
    settings: SettingsView,
    /// Per-remote mount config (cache mode, read-only, limits); cached from the
    /// DB, edited via the mount-options modal, read when mounting.
    mount_configs: HashMap<String, MountConfig>,
    /// Pane groups: one, or two side by side when split. Each owns its own tab
    /// strip and tabs. Always non-empty; capped at two.
    groups: Vec<PaneGroup>,
    /// Index of the focused group in `groups` (keyboard + preview target).
    active_group: usize,
    /// Left group's width fraction when split (the right group fills the rest).
    split_ratio: f32,
    /// Monotonic source of `Tab::id` (unique across groups).
    next_tab_id: usize,
    /// Spring-loaded tabs: a drag dwelling on a tab id activates it.
    spring: SpringLoad<usize>,
    /// Last folder viewed per remote; reopening a remote returns to it. Shared
    /// across tabs (a convenience cache, not part of any one browse context).
    remote_paths: HashMap<String, String>,
    copied: Option<CopySource>,
    /// Persisted user preferences (sort, refresh interval, rclone path, font size)
    /// — the source of truth backing the settings panel, written to disk on change.
    /// `SettingsView`'s input widgets are working copies of these values.
    store: SettingsStore,
    /// Cached window-layout state (sidebar/pane/dock widths, split ratio), distinct
    /// from `store`'s preferences; mirrored to `db` on change via [`Self::save_ui`].
    ui: UiState,
    pinned: Vec<String>,
    /// Recently-opened remote names (newest first); refreshed on navigate, read
    /// by the welcome screen — kept out of the render path.
    recent_remotes: Vec<String>,
    /// Names of mounted remotes; refreshed after a mount/unmount, read by the
    /// sidebar and remote menu — kept out of the render path.
    mounted: HashSet<String>,
    /// The single active modal overlay (palette, remote config, mount options,
    /// confirm). At most one is open; see [`ActiveModal`].
    modal: Option<ActiveModal>,
    prompt: Option<Entity<PromptModal>>,
    prompt_sub: Option<gpui::Subscription>,
    toasts: Entity<Toasts>,
    jobs: Entity<Jobs>,
    /// The active right-dock panel, if any (exclusive: preview xor tasks).
    dock: Option<Panel>,
    /// Right-dock width, shared by every panel (resizable; persisted).
    dock_width: Pixels,
    /// The preview panel (owns its subject, fetch, and cache). Workspace-level
    /// and re-targeted onto the focused pane's explorer on switch.
    preview: Entity<PreviewPane>,
    /// The rcd status item (owns daemon health + popover).
    daemon: Entity<DaemonStatus>,
    clipboard: Option<Clipboard>,
    /// The Tasks panel (own entity; owns its selection + focus). Reads `jobs`.
    tasks: Entity<TasksPane>,
    /// The Sync panel (compare/sync controls for a split), shown in a status-bar
    /// popover. Reads workspace state.
    sync_pane: Entity<SyncPane>,
    /// Last compare result (left vs right), shown as a summary + row markers.
    /// `None` until a compare runs; cleared when the split collapses.
    compare: Option<Vec<DiffEntry>>,
    /// A compare is in flight.
    comparing: bool,
    /// Chosen sync direction/mode and whether bisync should resync (first run).
    sync_mode: SyncMode,
    bisync_resync: bool,
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
        let ui = db.load_ui();
        let pinned = db.load_pinned();
        let recent_remotes = db.recent_remotes(RECENT_REMOTES_FETCH);
        let mount_configs = db
            .load_mount_configs()
            .into_iter()
            .filter_map(|(name, json)| serde_json::from_str(&json).ok().map(|c| (name, c)))
            .collect();
        let sidebar_width = clamped_width(ui.sidebar_width, SIDEBAR_W, SIDEBAR_MIN, SIDEBAR_MAX);
        let preview_width = clamped_width(ui.preview_width, PREVIEW_W, PREVIEW_MIN, PREVIEW_MAX);
        let col_date_width = clamped_width(ui.col_date_width, COL_DATE, COL_MIN, COL_MAX);
        let col_size_width = clamped_width(ui.col_size_width, COL_SIZE, COL_MIN, COL_MAX);
        // The dock shares one resizable width across panels (persisted as
        // `preview_width`); it always starts closed (no panel auto-opens).
        let dock_width = preview_width;
        let jobs = cx.new(|_| Jobs::new(service.clone()));
        jobs.update(cx, |jobs, cx| jobs.start_polling(cx));
        let refresh_field = cx.new(|cx| NumberField::new(store.get().refresh_secs, 1, 120, 1, cx));
        cx.subscribe(&refresh_field, |this, _, ev, cx| {
            let NumberFieldEvent::Changed(secs) = ev;
            this.set_refresh(*secs, cx);
        })
        .detach();
        let font_px = store.get().ui_font_size.round() as u64;
        let ui_font_field =
            cx.new(|cx| NumberField::new(font_px, UI_FONT_MIN as u64, UI_FONT_MAX as u64, 1, cx));
        cx.subscribe(&ui_font_field, |this, _, ev, cx| {
            let NumberFieldEvent::Changed(px) = ev;
            this.store.update(|s| s.ui_font_size = (*px as f32).clamp(UI_FONT_MIN, UI_FONT_MAX));
            cx.notify();
        })
        .detach();
        let toasts = cx.new(|cx| Toasts::new(window, cx));
        let weak = cx.entity().downgrade();
        let settings = store.get();
        let (sort_field, sort_order, refresh_secs) =
            (settings.sort_field, settings.sort_order, settings.refresh_secs);
        let sidebar = cx.new(|cx| Sidebar::new(weak.clone(), sidebar_width, cx));
        sidebar.focus_handle(cx).focus(window, cx);
        let sidebar_sub = cx.subscribe(&sidebar, Self::on_sidebar_event);
        let tab = Self::build_tab(
            0,
            &weak,
            &service,
            (sort_field, sort_order),
            refresh_secs,
            (col_date_width, col_size_width),
            window,
            cx,
        );
        // The first tab is active: only it polls its folder.
        tab.pane.explorer.update(cx, |e, cx| e.set_active(true, cx));
        // One shared preview, bound to the active pane's explorer (re-targeted on
        // tab switch / split focus change). The dock owns its width and visibility.
        let preview = cx
            .new(|cx| PreviewPane::new(weak.clone(), tab.pane.explorer.clone(), service.clone(), cx));
        let tasks = cx.new(|cx| TasksPane::new(weak.clone(), jobs.clone(), cx));
        let sync_pane = cx.new(|cx| SyncPane::new(weak.clone(), cx));
        let daemon = cx.new(|cx| DaemonStatus::new(weak.clone(), service.clone(), window, cx));
        // Re-render the status bar when the daemon's health changes.
        cx.observe(&daemon, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&jobs, |this, _, event, cx| match event {
            JobsEvent::Invalidate(dirs) => this.invalidate_dirs(dirs, cx),
            JobsEvent::Finished { verb, label, ok, error } => {
                if *ok {
                    // A completed bisync establishes the baseline; consume the one-shot
                    // resync so the next run reconciles instead of resyncing again.
                    if verb.as_ref() == SyncMode::Bisync.label() {
                        this.bisync_resync = false;
                    }
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
        let app = AppState { service, db, paths };
        let this = Self {
            app,
            version,
            focus,
            remotes: Vec::new(),
            sidebar,
            _sidebar_sub: sidebar_sub,
            menus: Menus::default(),
            settings: SettingsView {
                open: false,
                rclone_bin,
                rclone_edit: None,
                rclone_edit_focus: false,
                refresh_field,
                ui_font_field,
                storage_size: None,
                rclone_paths: None,
                rclone_cache_size: None,
            },
            mount_configs,
            groups: vec![PaneGroup::new(tab)],
            active_group: 0,
            split_ratio: 0.5,
            next_tab_id: 1,
            spring: SpringLoad::new(),
            remote_paths: HashMap::new(),
            copied: None,
            store,
            ui,
            pinned,
            recent_remotes,
            mounted: HashSet::new(),
            modal: None,
            prompt: None,
            prompt_sub: None,
            toasts,
            jobs,
            dock: None,
            dock_width,
            preview,
            daemon,
            clipboard: None,
            tasks,
            sync_pane,
            compare: None,
            comparing: false,
            sync_mode: SyncMode::Copy,
            bisync_resync: false,
        };
        this.load_remotes(cx);
        Self::check_updates_on_startup(cx);
        this
    }

    /// Build a fresh browse-context tab (welcome screen; no remote open). Shared
    /// by `new` (first tab) and the new-tab action.
    fn build_tab(
        id: usize,
        weak: &WeakEntity<Workspace>,
        service: &Service,
        sort: (SortField, SortOrder),
        refresh_secs: u64,
        cols: (Pixels, Pixels),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Tab {
        let pane = Pane::new(weak, service, sort, refresh_secs, cols, window, cx);
        Tab { id, pinned: false, pane }
    }

    fn active_group(&self) -> &PaneGroup {
        &self.groups[self.active_group]
    }

    fn active_group_mut(&mut self) -> &mut PaneGroup {
        &mut self.groups[self.active_group]
    }

    /// The focused group's active tab.
    fn active(&self) -> &Tab {
        self.active_group().active_tab()
    }

    fn active_mut(&mut self) -> &mut Tab {
        self.active_group_mut().active_tab_mut()
    }

    /// The focused tab's pane.
    fn focused_pane(&self) -> &Pane {
        &self.active().pane
    }

    fn focused_pane_mut(&mut self) -> &mut Pane {
        &mut self.active_mut().pane
    }

    /// The active tab's focused explorer (cheap `Arc` clone; lets call sites
    /// read/update without holding a borrow on the workspace).
    fn explorer(&self) -> Entity<Explorer> {
        self.focused_pane().explorer.clone()
    }

    fn action_bar(&self) -> Entity<ActionBar> {
        self.focused_pane().action_bar.clone()
    }

    fn preview(&self) -> Entity<PreviewPane> {
        self.preview.clone()
    }

    /// Bridge explorer signals to navigation, preview, menus, and file ops —
    /// handled after the explorer's own update completes, so calling back into
    /// the explorer here never re-enters its borrow.
    fn on_explorer_event(&mut self, _: Entity<Explorer>, event: &ExplorerEvent, cx: &mut Context<Self>) {
        match event {
            ExplorerEvent::OpenDir(path) => {
                let remote = self.focused_pane().open_remote.clone().unwrap_or_default();
                self.navigate(remote, path.clone(), None, cx);
            }
            ExplorerEvent::OpenFile => self.open_preview(cx),
            ExplorerEvent::Context(entry, pos) => {
                self.menus.bg_menu = None;
                self.menus.context = Some((entry.clone(), *pos));
                cx.notify();
            }
            ExplorerEvent::Background(pos) => {
                self.menus.context = None;
                self.menus.bg_menu = Some(*pos);
                cx.notify();
            }
            ExplorerEvent::Upload(paths) => self.upload_paths(paths.clone(), cx),
            ExplorerEvent::Drop { dragged, dst_remote, dst_dir, mods } => {
                self.drop_into(dragged, dst_remote.clone(), dst_dir.clone(), *mods, cx);
            }
            ExplorerEvent::SortChanged(field, order) => {
                let (field, order) = (*field, *order);
                self.store.update(|s| {
                    s.sort_field = field;
                    s.sort_order = order;
                });
            }
        }
    }

    /// Bridge sidebar signals to the remotes model and navigation.
    fn on_sidebar_event(&mut self, _: Entity<Sidebar>, event: &SidebarEvent, cx: &mut Context<Self>) {
        match event {
            SidebarEvent::Open(ix) => self.load_remote(*ix, cx),
            SidebarEvent::Menu(name, pos) => {
                self.menus.remote_menu = Some((name.clone(), *pos));
                cx.notify();
            }
            SidebarEvent::Add => self.begin_add_remote(cx),
            SidebarEvent::Reorder { from, before } => self.reorder_pinned(from, before, cx),
            SidebarEvent::DropEntry { dragged, dst_remote, mods } => {
                self.drop_into(dragged, dst_remote.clone(), String::new(), *mods, cx);
            }
        }
    }

}
