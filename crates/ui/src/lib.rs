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
mod preview;
mod query;
mod remotes;
mod sidebar;
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
    Menu, MenuItem,
    MouseButton, MouseDownEvent, MouseUpEvent,
    PathPromptOptions, Pixels, Point, ScrollStrategy, SharedString, Stateful, TitlebarOptions,
    UniformListScrollHandle, Window, WindowBounds, WindowOptions,
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
        ShowKeybindings
    ]
);

pub use bootstrap::{run, RcloneStatus, Startup};
pub(crate) use status_screen::{relaunch, StatusScreen};

#[derive(PartialEq, Clone, Copy)]
enum CopySource {
    Path,
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

/// Occupant of the single right-side dock. At most one is shown; toggling one
/// replaces the other (Zed-style exclusive dock). Each panel is rendered by its
/// owner — Preview by the explorer view (so it can't exist without an open
/// remote), Tasks by the workspace (global).
#[derive(Clone, Copy, PartialEq)]
enum DockPanel {
    Preview,
    Tasks,
}

#[derive(Clone, Copy, PartialEq)]
enum ResizeTarget {
    Sidebar,
    Preview,
    Jobs,
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
#[derive(Clone)]
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

/// The app's shared, cheap-to-clone dependencies, bundled so the workspace holds
/// one field instead of three; cloned out to async tasks as needed.
#[derive(Clone)]
struct AppState {
    service: Service,
    db: Db,
    paths: Paths,
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
    open_remote: Option<String>,
    /// Empty = root.
    path: String,
    /// The file-list pane: owns the listing, selection, search, and sort.
    explorer: Entity<Explorer>,
    _explorer_sub: gpui::Subscription,
    history: Vec<Location>,
    history_pos: usize,
    /// Last folder viewed per remote; reopening a remote returns to it.
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
    dock: Option<DockPanel>,
    /// Tasks pane width (resizable; persisted). Reuses the preview clamp range.
    jobs_width: Pixels,
    /// The preview pane (owns its subject, fetch, and cache); rendered inside the
    /// explorer column.
    preview: Entity<PreviewPane>,
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
        let jobs_width = clamped_width(ui.jobs_width, JOBS_W, PREVIEW_MIN, PREVIEW_MAX);
        // Preview is the only persisted dock choice; tasks always starts closed.
        let dock = ui.preview_open.then_some(DockPanel::Preview);
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
        let explorer = cx.new(|cx| {
            Explorer::new(
                weak.clone(),
                service.clone(),
                (sort_field, sort_order),
                refresh_secs,
                (col_date_width, col_size_width),
                window,
                cx,
            )
        });
        let explorer_sub = cx.subscribe(&explorer, Self::on_explorer_event);
        let sidebar = cx.new(|cx| Sidebar::new(weak.clone(), sidebar_width, cx));
        sidebar.focus_handle(cx).focus(window, cx);
        let sidebar_sub = cx.subscribe(&sidebar, Self::on_sidebar_event);
        let preview = cx.new(|cx| {
            PreviewPane::new(weak.clone(), explorer.clone(), service.clone(), preview_width, cx)
        });
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
            open_remote: None,
            path: String::new(),
            explorer,
            _explorer_sub: explorer_sub,
            history: Vec::new(),
            history_pos: 0,
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
            dock,
            jobs_width,
            preview,
            daemon,
            clipboard: None,
        };
        this.load_remotes(cx);
        this
    }

    /// Bridge explorer signals to navigation, preview, menus, and file ops —
    /// handled after the explorer's own update completes, so calling back into
    /// the explorer here never re-enters its borrow.
    fn on_explorer_event(&mut self, _: Entity<Explorer>, event: &ExplorerEvent, cx: &mut Context<Self>) {
        match event {
            ExplorerEvent::OpenDir(path) => {
                let remote = self.open_remote.clone().unwrap_or_default();
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
            ExplorerEvent::Drop { dragged, dst_remote, dst_dir, copy } => {
                self.drop_into(dragged, dst_remote.clone(), dst_dir.clone(), *copy, cx);
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
            SidebarEvent::DropEntry { dragged, dst_remote } => {
                self.drop_into(dragged, dst_remote.clone(), String::new(), false, cx);
            }
        }
    }

}
