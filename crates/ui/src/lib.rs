//! gpui desktop shell: a two-pane remote browser.

mod command_palette;
mod components;
mod daemon;
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
mod remotes;
mod sidebar;
mod spring;
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
    ArgKind, ArgSpec, ArgValue, ConfigPaths, Entry, InfoOp, InfoResult, Matcher, MountConfig,
    Operation, Provider, RemoteInfo, RemoteOption, Service, ServiceError, TransferMode,
};

use preview::PreviewPane;
use action_bar::ActionBar;
use spring::SpringLoad;
use command_palette::CommandPaletteDelegate;
use confirm::ConfirmModal;
use keybindings::KeybindingsView;
use picker::Picker;
use prompt::PromptModal;
use toast::{ToastBody, Toasts};
use workspace::modal::ActiveModal;
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
        TogglePalette,
        AddRemote,
        OpenSettings,
        RestartDaemon,
        ToggleTasks,
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

/// Settings-panel state, grouped off the workspace. The panel is rendered by
/// `panels::settings` and orchestrated by `workspace::settings`.
struct SettingsView {
    open: bool,
    /// The rclone binary path in use (shown in the rclone setting).
    rclone_bin: String,
    /// In-progress rclone binary/config override edit (field + input).
    rclone_edit: Option<(RcloneField, Entity<TextInput>)>,
    /// Focus the rclone edit input once when it opens (not every frame, which
    /// would trap focus in it).
    rclone_edit_focus: bool,
    refresh_field: Entity<NumberField>,
    /// UI font-size stepper in px (mirrors `ui_font_size`).
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
    /// Task-row right-click menu: the row's actions and the cursor position.
    task_menu: Option<(TaskMenuData, Point<Pixels>)>,
    /// The rc-daemon health popover (status bar).
    rc_popover_open: bool,
}

/// What a Tasks-row context menu acts on — captured at right-click.
#[derive(Clone)]
struct TaskMenuData {
    job_id: usize,
    command: String,
    targets: Vec<JobTarget>,
    running: bool,
    can_retry: bool,
    can_remove: bool,
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
    Dock,
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

/// Rubber-band selection in the file list. Carries no state — the band's anchor
/// and live selection live on the [`Explorer`]; this just drives gpui's drag
/// lifecycle (press-move-release) and renders no preview.
#[derive(Clone)]
struct DragMarquee;

impl Render for DragMarquee {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

struct DraggedRemote {
    name: String,
}

/// A tab being dragged to reorder it within the strip.
#[derive(Clone)]
struct DraggedTab {
    id: usize,
    title: SharedString,
}

/// One entry inside a [`DraggedEntry`].
#[derive(Clone)]
struct DragItem {
    path: String,
    name: String,
    is_dir: bool,
}

/// A drag from the file list — self-contained so a drop is correct anywhere,
/// regardless of the active tab. `remote` and `items` are snapshotted at drag
/// start: `items` is the whole selection (or the single dragged row), each with
/// its full path.
#[derive(Clone)]
struct DraggedEntry {
    remote: String,
    items: Vec<DragItem>,
}

struct DragLabel {
    text: SharedString,
    /// Cursor position within the grabbed element at drag start. gpui paints the
    /// drag preview at `cursor - offset`, so shifting the label back by `offset`
    /// re-anchors it to the cursor regardless of where a wide row was grabbed.
    offset: Point<Pixels>,
}

impl DragLabel {
    fn new(text: impl Into<SharedString>, offset: Point<Pixels>) -> Self {
        Self { text: text.into(), offset }
    }
}

impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().pl(self.offset.x + px(12.0)).pt(self.offset.y + px(8.0)).child(
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(rgb(ELEVATED))
                .shadow_lg()
                .text_xs()
                .text_color(rgb(FG))
                .child(self.text.clone()),
        )
    }
}

/// The app's shared, cheap-to-clone dependencies, bundled so the workspace holds
/// one field instead of three; cloned out to async tasks as needed.
#[derive(Clone)]
struct AppState {
    service: Service,
    db: Db,
    paths: Paths,
}

/// One browse context — the unit a tab owns. Everything here is independent per
/// tab: the open location, the file-list pane (selection/search/sort/scroll),
/// and back/forward history. The workspace holds a `Vec<Tab>` and renders the
/// active one. The split point for future side-by-side panes is to group these
/// under a `Pane`; nothing in `Tab` would change.
struct Tab {
    /// Stable identity (survives reordering on pin/unpin); used to track the
    /// active tab and to target context-menu actions.
    id: usize,
    /// Session-only pin. Pinned tabs sort before unpinned, render compact with no
    /// close button, and close only via the context menu.
    pinned: bool,
    open_remote: Option<String>,
    /// Empty = root.
    path: String,
    /// The file-list pane: owns the listing, selection, search, and sort.
    explorer: Entity<Explorer>,
    /// Routes this explorer's events to the workspace; dropped when the tab closes.
    _explorer_sub: gpui::Subscription,
    history: Vec<Location>,
    history_pos: usize,
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

impl Panel {
    fn title(self) -> &'static str {
        match self {
            Panel::Preview => "PREVIEW",
            Panel::Tasks => "TASKS",
        }
    }
}

struct Workspace {
    app: AppState,
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
    /// Open tabs; each is an independent browse context. Always non-empty.
    tabs: Vec<Tab>,
    /// Index of the active tab in `tabs`.
    active: usize,
    /// Monotonic source of `Tab::id`.
    next_tab_id: usize,
    /// Spring-loaded tabs: a drag dwelling on a tab id activates it.
    spring: SpringLoad<usize>,
    /// Horizontal scroll offset of the tab strip (persists across frames so the
    /// strip stays put when tabs overflow).
    tab_scroll: ScrollHandle,
    /// Last folder viewed per remote; reopening a remote returns to it. Shared
    /// across tabs (a convenience cache, not part of any one browse context).
    remote_paths: HashMap<String, String>,
    copied: Option<CopySource>,
    store: SettingsStore,
    /// Cached layout state, mirrored to `db` on change via [`Self::save_ui`].
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
    /// and re-targeted onto the active tab's explorer on switch.
    preview: Entity<PreviewPane>,
    /// The breadcrumb path bar (its own entity for a definite width). Re-targeted
    /// onto the active tab's explorer on switch.
    action_bar: Entity<ActionBar>,
    /// The rcd status item (owns daemon health + popover).
    daemon: Entity<DaemonStatus>,
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
        tab.explorer.update(cx, |e, cx| e.set_active(true, cx));
        // One shared preview, bound to the active tab's explorer (re-targeted on
        // tab switch). The dock owns its width and visibility.
        let preview =
            cx.new(|cx| PreviewPane::new(weak.clone(), tab.explorer.clone(), service.clone(), cx));
        let action_bar = cx.new(|cx| ActionBar::new(weak.clone(), tab.explorer.clone(), cx));
        let daemon = cx.new(|cx| DaemonStatus::new(weak.clone(), service.clone(), window, cx));
        // Re-render the status bar when the daemon's health changes.
        cx.observe(&daemon, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&jobs, |this, _, event, cx| match event {
            JobsEvent::ReloadEntries => this.force_reload_entries(cx),
            JobsEvent::Finished { label, ok, error } => {
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
            tabs: vec![tab],
            active: 0,
            next_tab_id: 1,
            spring: SpringLoad::new(),
            tab_scroll: ScrollHandle::new(),
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
            action_bar,
            daemon,
            clipboard: None,
        };
        this.load_remotes(cx);
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
        let explorer = cx.new(|cx| {
            Explorer::new(weak.clone(), service.clone(), sort, refresh_secs, cols, window, cx)
        });
        let explorer_sub = cx.subscribe(&explorer, Self::on_explorer_event);
        Tab {
            id,
            pinned: false,
            open_remote: None,
            path: String::new(),
            explorer,
            _explorer_sub: explorer_sub,
            history: Vec::new(),
            history_pos: 0,
        }
    }

    fn active(&self) -> &Tab {
        &self.tabs[self.active]
    }

    fn active_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    /// The active tab's explorer (cheap `Arc` clone; lets call sites read/update
    /// without holding a borrow on the workspace).
    fn explorer(&self) -> Entity<Explorer> {
        self.active().explorer.clone()
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
                let remote = self.active().open_remote.clone().unwrap_or_default();
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
