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

// Re-exported at the crate root so `use <c>::…` resolves from anywhere.
pub(crate) use components::{confirm, context_menu, image_view, number_field, picker, prompt, text_input, toast, widgets};
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
use context_menu::{ContextMenu, MenuHeader, MenuSpec};
use transfers::{Job, JobTarget, Jobs, JobsEvent};
use number_field::{NumberField, NumberFieldEvent};
use explorer::{Explorer, ExplorerEvent};
use sidebar::{Sidebar, SidebarEvent};
use daemon::{DaemonStatus, RcHealth};
use text_input::TextInput;
use query::{Query, Status};
use theme::*;
use widgets::*;

/// Frecency-ranked remotes fetched from the cache; the welcome screen filters
/// these against the live config and shows the first few, so over-fetch to
/// survive remotes that were since deleted.
const FREQUENT_REMOTES_FETCH: usize = 20;
const FREQUENT_REMOTES_SHOWN: usize = 5;

/// App version — the shared workspace version (`cargo release` bumps it, the
/// `vX.Y.Z` tag matches it).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

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
        PrevTab
    ]
);

/// Jump to tab `n` (1-based) in the focused group; `n >= 9` means the last tab.
#[derive(Clone, PartialEq, Default, Debug, gpui::Action)]
#[action(namespace = rspace, no_json)]
pub struct ActivateTab(pub usize);

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
/// preferences live in [`Workspace::store`]; this holds only edit buffers (seeded
/// from `store`, written back on commit) and read-only fetches for display.
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

/// The one transient popover/menu open over the workspace — at most one at a
/// time (every opener calls [`Workspace::close_menus`] first, so this is a type
/// invariant). Cleared as a unit by `close_menus`.
enum ActiveMenu {
    /// A right-click menu: the focusable [`ContextMenu`] and where it is anchored.
    /// Its rows are snapshotted at open time, like Zed's.
    Items(Entity<ContextMenu>, Point<Pixels>, gpui::Subscription),
    /// The rc-daemon health popover, anchored to its status-bar button rather
    /// than the cursor, so it renders there instead of via `render_active_menu`.
    RcPopover(Entity<ContextMenu>, gpui::Subscription),
    /// The sync compare/sync popover (status bar).
    SyncPopover,
}

/// Source for a cross-remote copy/cut, resolved against the destination at paste.
#[derive(Clone)]
struct Clipboard {
    remote: String,
    entries: Vec<Entry>,
    mode: TransferMode,
}

/// The right dock: which panel is shown (if any) and its shared, persisted width.
struct Dock {
    panel: Option<Panel>,
    width: Pixels,
}

/// Split-view compare/sync state — only meaningful with two panes open.
struct SyncState {
    /// Last compare result (left vs right); `None` until a compare runs, cleared
    /// when the split collapses.
    result: Option<Vec<DiffEntry>>,
    /// A compare is in flight.
    comparing: bool,
    /// Chosen sync direction/mode.
    mode: SyncMode,
    /// Whether bisync should resync (first run establishes the baseline).
    bisync_resync: bool,
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
    pane: Entity<Pane>,
    /// Workspace-side subscription to the pane's explorer events; tied to the
    /// tab's lifetime so it drops when the tab closes.
    _explorer_sub: gpui::Subscription,
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
    /// The one open right-click menu / popover, if any (see [`ActiveMenu`]).
    menu: Option<ActiveMenu>,
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
    /// Cached handles of the active pane's explorer/action bar, refreshed on
    /// every active-tab change so `explorer()`/`action_bar()` stay borrow-free.
    active_explorer: Entity<Explorer>,
    active_action_bar: Entity<ActionBar>,
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
    frequent_remotes: Vec<String>,
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
    /// The right dock: shown panel (exclusive: preview xor tasks) + its width.
    dock: Dock,
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
    /// Split-view compare/sync state (see [`SyncState`]).
    sync: SyncState,
    /// Whether our window is frontmost — gates OS notifications so a finished
    /// transfer only pings the system tray when the user isn't already looking.
    window_active: bool,
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
        let frequent_remotes = db.frequent_remotes(FREQUENT_REMOTES_FETCH);
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
        let (first_explorer, first_action_bar) = {
            let pane = tab.pane.read(cx);
            (pane.explorer.clone(), pane.action_bar.clone())
        };
        // The first tab is active: only it polls its folder.
        first_explorer.update(cx, |e, cx| e.set_active(true, cx));
        // One shared preview, bound to the active pane's explorer (re-targeted on
        // tab switch / split focus change). The dock owns its width and visibility.
        let preview = cx
            .new(|cx| PreviewPane::new(weak.clone(), first_explorer.clone(), service.clone(), cx));
        let tasks = cx.new(|cx| TasksPane::new(weak.clone(), jobs.clone(), cx));
        let sync_pane = cx.new(|cx| SyncPane::new(weak.clone(), cx));
        let daemon = cx.new(|cx| DaemonStatus::new(weak.clone(), service.clone(), window, cx));
        // Re-render the status bar when the daemon's health changes.
        cx.observe(&daemon, |_, _, cx| cx.notify()).detach();
        // Track frontmost state to gate OS notifications (see `window_active`).
        cx.observe_window_activation(window, |this, window, _| {
            this.window_active = window.is_window_active();
        })
        .detach();
        // Clicking a transfer notification brings the app forward.
        cx.on_system_notification_response(|_, cx| {
            if let Some(handle) = cx.active_window() {
                handle.update(cx, |_, window, _| window.activate_window()).ok();
            }
        });
        cx.subscribe(&jobs, |this, _, event, cx| match event {
            JobsEvent::Invalidate(dirs) => this.invalidate_dirs(dirs, cx),
            JobsEvent::Finished { verb, label, ok, error } => {
                if *ok {
                    // A completed bisync establishes the baseline; consume the one-shot
                    // resync so the next run reconciles instead of resyncing again.
                    if verb.as_ref() == SyncMode::Bisync.label() {
                        this.sync.bisync_resync = false;
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
                this.notify_transfer(label, *ok, error.as_ref(), cx);
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
            menu: None,
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
            active_explorer: first_explorer,
            active_action_bar: first_action_bar,
            split_ratio: 0.5,
            next_tab_id: 1,
            spring: SpringLoad::new(),
            remote_paths: HashMap::new(),
            copied: None,
            store,
            ui,
            pinned,
            frequent_remotes,
            mounted: HashSet::new(),
            modal: None,
            prompt: None,
            prompt_sub: None,
            toasts,
            jobs,
            dock: Dock { panel: None, width: dock_width },
            preview,
            daemon,
            clipboard: None,
            tasks,
            sync_pane,
            sync: SyncState {
                result: None,
                comparing: false,
                mode: SyncMode::Copy,
                bisync_resync: false,
            },
            window_active: true,
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
        let (pane, explorer_sub) = Pane::new(weak, service, sort, refresh_secs, cols, window, cx);
        Tab { id, pinned: false, pane, _explorer_sub: explorer_sub }
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

    /// The focused tab's pane entity (cheap handle; always the current active tab).
    fn focused_pane(&self) -> Entity<Pane> {
        self.active().pane.clone()
    }

    /// The focused pane's open remote / path, read through the pane entity.
    fn open_remote(&self, cx: &App) -> Option<String> {
        self.focused_pane().read(cx).open_remote.clone()
    }

    fn open_path(&self, cx: &App) -> String {
        self.focused_pane().read(cx).path.clone()
    }

    /// The active pane's explorer/action bar. Cached handles, refreshed by
    /// [`Self::sync_active_handles`] on every active-tab change, so these stay
    /// cheap and borrow-free at their many call sites.
    fn explorer(&self) -> Entity<Explorer> {
        self.active_explorer.clone()
    }

    fn action_bar(&self) -> Entity<ActionBar> {
        self.active_action_bar.clone()
    }

    /// Re-cache the active pane's explorer/action-bar handles. Call after any
    /// change to which tab/group is active.
    fn sync_active_handles(&mut self, cx: &App) {
        let pane = self.active().pane.clone();
        let pane = pane.read(cx);
        self.active_explorer = pane.explorer.clone();
        self.active_action_bar = pane.action_bar.clone();
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
                let remote = self.open_remote(cx).unwrap_or_default();
                self.navigate(remote, path.clone(), None, cx);
            }
            ExplorerEvent::OpenFile => self.open_preview(cx),
            ExplorerEvent::Context(entry, pos) => {
                let spec = self.entry_menu_spec(entry.clone(), cx);
                self.open_menu(spec, *pos, cx);
            }
            ExplorerEvent::Background(pos) => {
                let spec = self.bg_menu_spec(cx);
                self.open_menu(spec, *pos, cx);
            }
            ExplorerEvent::Upload { paths, dst_remote, dst_dir } => {
                self.upload_paths(paths.clone(), dst_remote.clone(), dst_dir.clone(), cx)
            }
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
                let spec = self.remote_menu_spec(name.clone());
                self.open_menu(spec, *pos, cx);
            }
            SidebarEvent::Add => self.begin_add_remote(cx),
            SidebarEvent::Reorder { from, before } => self.reorder_pinned(from, before, cx),
            SidebarEvent::DropEntry { dragged, dst_remote, mods } => {
                self.drop_into(dragged, dst_remote.clone(), String::new(), *mods, cx);
            }
        }
    }

}
