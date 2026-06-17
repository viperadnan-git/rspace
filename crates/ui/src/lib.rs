//! gpui desktop shell: a two-pane remote browser.

mod command_palette;
mod confirm;
mod fuzzy;
mod jobs;
mod menus;
mod panels;
mod picker;
mod preview;
mod prompt;
mod query;
mod remotes;
mod text_input;
mod theme;
mod toast;
mod transfers;
mod views;
mod widgets;

use std::collections::HashSet;
use std::ops::Range;
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
use rspace_core::{dir_size, Db, JobRecord, Paths, SettingsStore, SortField, SortOrder, UiState};
use rspace_rclone_rc::{
    ArgKind, ArgSpec, ArgValue, Entry, InfoOp, InfoResult, Operation, Provider, RemoteInfo,
    RemoteOption, Service, ServiceError, TransferMode,
};

use preview::{Preview, PreviewState};
use command_palette::CommandPaletteDelegate;
use confirm::ConfirmModal;
use picker::Picker;
use prompt::PromptModal;
use toast::{Toast, ToastBody};
use transfers::{Job, JobTarget, Jobs, JobsEvent};
use remotes::RemoteConfigModal;
use query::{Query, Status};
use theme::*;
use widgets::*;

/// Rows shown in the transfers history view.
const JOB_HISTORY_LIMIT: usize = 50;
/// Recent remotes fetched into the cache; the welcome screen filters these
/// against the live config and shows the first few, so over-fetch to survive
/// remotes that were since deleted.
const RECENT_REMOTES_FETCH: usize = 20;
/// Recent remotes shown on the welcome screen.
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
        ToggleTransfers
    ]
);

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        // Brand mark (transparent; tinted by svg()). app-icon.png/.icns derive
        // from it via scripts/make_icns.sh.
        if path == "logo.svg" {
            return Ok(Some(std::borrow::Cow::Borrowed(
                include_bytes!("../../app/resources/logo.svg").as_slice(),
            )));
        }
        // Each name maps `icons/<name>.svg` to the embedded bytes; add one word here.
        macro_rules! icons {
            ($($name:literal),* $(,)?) => {
                match path {
                    $(concat!("icons/", $name, ".svg") => Some(std::borrow::Cow::Borrowed(
                        include_bytes!(concat!("../assets/icons/", $name, ".svg")).as_slice(),
                    )),)*
                    _ => None,
                }
            };
        }
        Ok(icons!(
            "folder", "file", "copy", "check", "settings", "alert", "maximize", "minimize", "download",
            "upload", "folder_open", "pin", "chevron_up", "chevron_down", "scissors", "clipboard",
            "refresh", "activity", "trash", "x", "edit", "cloud", "hard_drive", "server", "database",
            "lock", "image", "drive", "dropbox", "gcs", "b2", "box", "mega", "swift",
            "yandex", "nextcloud", "protondrive", "icloud", "onedrive", "s3", "azureblob", "smb",
            "googlephotos", "internetarchive", "zoho", "seafile", "mailru", "sharefile", "memory",
            "cache", "compress", "chunker", "union", "alias", "hasher", "owncloud", "sidebar_right",
            "plus", "server_network", "server_network_off"
        ))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

pub enum RcloneStatus {
    Found { path: String, version: String },
    Missing { install_url: String },
    Error { message: String },
}

/// Startup state. `service` is present only when the daemon started.
pub struct Startup {
    pub rclone: RcloneStatus,
    pub service: Option<Service>,
    pub paths: Paths,
    pub store: SettingsStore,
    pub db: Db,
}

/// Launch the desktop shell. Blocks until the app exits.
pub fn run(startup: Startup) {
    application().with_assets(Assets).run(move |cx: &mut App| {
        bind_keys(cx);
        text_input::bind_keys(cx);
        picker::bind_keys(cx);
        cx.set_menus(vec![
            Menu::new("rspace").items([MenuItem::action("Quit rspace", Quit)]),
            Menu::new("Window").items([
                MenuItem::action("Minimize", Minimize),
                MenuItem::action("Zoom", Zoom),
                MenuItem::action("Toggle Full Screen", ToggleFullscreen),
            ]),
        ]);
        cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1000.0), px(640.0)), cx);
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: None,
                appears_transparent: true,
                traffic_light_position: Some(point(px(9.0), px(9.0))),
            }),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };

        let Startup { rclone, service, paths, store, db } = startup;
        match service {
            Some(service) => {
                let version = match &rclone {
                    RcloneStatus::Found { version, .. } => version.clone(),
                    _ => String::new(),
                };
                cx.open_window(options, |window, cx| {
                    cx.new(|cx| Workspace::new(service, version, paths, store, db, window, cx))
                })
                .unwrap();
            }
            None => {
                cx.open_window(options, |_, cx| cx.new(|_| StatusScreen { rclone }))
                    .unwrap();
            }
        }
        cx.activate(true);
    });
}

fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-q", Quit, None),
        // macOS native chord + the F11 convention used on Linux/Windows.
        KeyBinding::new("ctrl-cmd-f", ToggleFullscreen, None),
        KeyBinding::new("f11", ToggleFullscreen, None),
        // Navigation/mutation are inert while a confirm dialog owns the keyboard.
        KeyBinding::new("down", SelectNext, Some("Workspace && !modal")),
        KeyBinding::new("j", SelectNext, Some("Workspace && !modal")),
        KeyBinding::new("up", SelectPrev, Some("Workspace && !modal")),
        KeyBinding::new("k", SelectPrev, Some("Workspace && !modal")),
        // Shift extends the selection (handler reads live modifiers).
        KeyBinding::new("shift-down", SelectNext, Some("Workspace && !modal")),
        KeyBinding::new("shift-j", SelectNext, Some("Workspace && !modal")),
        KeyBinding::new("shift-up", SelectPrev, Some("Workspace && !modal")),
        KeyBinding::new("shift-k", SelectPrev, Some("Workspace && !modal")),
        KeyBinding::new("secondary-a", SelectAll, Some("Workspace && !modal")),
        KeyBinding::new("enter", Open, Some("Workspace && !modal")),
        KeyBinding::new("tab", TogglePane, Some("Workspace && !modal")),
        KeyBinding::new("backspace", GoUp, Some("Workspace && !modal")),
        KeyBinding::new("secondary-[", GoBack, Some("Workspace && !modal")),
        KeyBinding::new("secondary-]", GoForward, Some("Workspace && !modal")),
        KeyBinding::new("secondary-r", Reload, Some("Workspace && !modal")),
        // Toggle (not !modal) so it can also close itself; the handler ignores
        // it while another modal is open.
        // The modern "cmdk" command-menu shortcut: cmd-k on macOS, ctrl-k elsewhere.
        KeyBinding::new("secondary-k", TogglePalette, Some("Workspace")),
        KeyBinding::new("left", FocusSidebar, Some("Workspace && !modal")),
        KeyBinding::new("right", FocusExplorer, Some("Workspace && !modal")),
        KeyBinding::new("secondary-c", CopyEntry, Some("Workspace && !modal")),
        KeyBinding::new("secondary-x", CutEntry, Some("Workspace && !modal")),
        KeyBinding::new("secondary-v", PasteEntry, Some("Workspace && !modal")),
        KeyBinding::new("secondary-backspace", DeleteEntry, Some("Workspace && !modal")),
        KeyBinding::new("secondary-shift-n", NewFolder, Some("Workspace && !modal")),
        KeyBinding::new("secondary-u", NewFile, Some("Workspace && !modal")),
        KeyBinding::new("f2", Rename, Some("Workspace && !modal")),
        KeyBinding::new("space", TogglePreview, Some("Workspace && !modal")),
        KeyBinding::new("escape", CloseSettings, Some("Workspace")),
        // Add/edit-remote dialog: arrows (or ctrl-n/p) navigate the picker,
        // Enter advances. Bound to its own context so any focusable list can reuse.
        KeyBinding::new("down", ConfigNext, Some("RemoteConfig")),
        KeyBinding::new("ctrl-n", ConfigNext, Some("RemoteConfig")),
        KeyBinding::new("up", ConfigPrev, Some("RemoteConfig")),
        KeyBinding::new("ctrl-p", ConfigPrev, Some("RemoteConfig")),
        // Enter confirms only when a text field is focused; focused buttons/toggles
        // get gpui's native Enter/Space activation, so this would otherwise double-fire.
        KeyBinding::new("enter", ConfigConfirm, Some("RemoteConfig > TextInput")),
        KeyBinding::new("tab", FocusNext, Some("RemoteConfig")),
        KeyBinding::new("shift-tab", FocusPrev, Some("RemoteConfig")),
        // Confirm dialog: Enter accepts (Escape dismisses via the line above).
        KeyBinding::new("enter", ConfirmAccept, Some("Confirm")),
        // Text-input dialog: Enter submits, Escape cancels.
        KeyBinding::new("enter", PromptSubmit, Some("Prompt")),
        KeyBinding::new("escape", PromptCancel, Some("Prompt")),
    ]);
    // Minimize is a macOS app convention (cmd-m); elsewhere the window manager owns it.
    #[cfg(target_os = "macos")]
    cx.bind_keys([KeyBinding::new("cmd-m", Minimize, None)]);
}

struct StatusScreen {
    rclone: RcloneStatus,
}

impl Render for StatusScreen {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let message: SharedString = match &self.rclone {
            RcloneStatus::Found { path, version } => format!("{version}\n{path}").into(),
            RcloneStatus::Missing { install_url } => {
                format!("rclone not found — install it from {install_url}").into()
            }
            RcloneStatus::Error { message } => format!("rclone error\n{message}").into(),
        };
        v_flex()
            .size_full()
            .gap_4()
            .bg(rgb(CANVAS))
            .text_color(rgb(FG))
            .justify_center()
            .items_center()
            .child(div().text_2xl().child("rspace"))
            .child(div().text_sm().text_color(rgb(FG_MUTED)).child(message))
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Pane {
    Sidebar,
    Explorer,
}

/// Identifies which copy button is showing its "copied" check.
#[derive(PartialEq, Clone, Copy)]
enum CopySource {
    Path,
    Error,
    JobCommand(usize),
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
    /// Status icon: a slashed server when unreachable, a plain one otherwise.
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
    /// Name of the row selected here, restored by identity on return.
    selected: Option<String>,
}

/// Source for a cross-remote copy/cut, resolved against the destination at paste.
#[derive(Clone)]
struct Clipboard {
    remote: String,
    entries: Vec<Entry>,
    mode: TransferMode,
}

/// Which side pane a resize drag is adjusting.
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

/// A resizable file-list column (Name flex-grows and isn't resizable).
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

/// A pinned remote being dragged to reorder; the name identifies the source.
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

/// The floating label rendered under the cursor while dragging.
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

/// Two-pane remote browser.
struct Workspace {
    service: Service,
    version: String,
    focus: FocusHandle,
    pane: Pane,
    remotes: Vec<RemoteInfo>,
    remote_sel: usize,
    remote_scroll: UniformListScrollHandle,
    /// Right-click menu on a remote: the remote name and the cursor position.
    remote_menu: Option<(String, Point<Pixels>)>,
    /// Open add/edit-remote modal (schema-driven, backend-agnostic).
    remote_config: Option<Entity<RemoteConfigModal>>,
    /// Subscription to the open modal's dismiss/saved events.
    remote_config_sub: Option<gpui::Subscription>,
    sidebar_width: Pixels,
    preview_width: Pixels,
    col_date_width: Pixels,
    col_size_width: Pixels,
    open_remote: Option<String>,
    /// Empty = root.
    path: String,
    /// Cursor (lead) row index.
    entry_sel: usize,
    /// Multi-selection by entry path; survives re-sort and refresh. Always
    /// contains the cursor's path unless explicitly toggled off.
    selected: HashSet<String>,
    /// Anchor index for shift-range selection.
    sel_anchor: usize,
    entry_scroll: UniformListScrollHandle,
    /// A row to select by name once the next listing loads (e.g. the child
    /// folder after navigating up).
    pending_select: Option<String>,
    dir_query: Query<(String, String), Vec<Entry>>,
    history: Vec<Location>,
    history_pos: usize,
    /// Which copy button last fired, so only that one shows the check.
    copied: Option<CopySource>,
    sort_field: SortField,
    sort_order: SortOrder,
    paths: Paths,
    /// User preferences (settings.json).
    store: SettingsStore,
    /// App-managed state + history (state/rspace.db).
    db: Db,
    /// Cached layout state, mirrored to `db` on change via [`Self::save_ui`].
    ui: UiState,
    /// Pinned remote names (display order); persisted to `db`'s pinned table.
    pinned: Vec<String>,
    /// Recently-opened remote names (newest first); refreshed on navigate, read
    /// by the welcome screen — kept out of the render path.
    recent_remotes: Vec<String>,
    /// Cached job log for the transfers history view; refreshed on `Logged`.
    job_history: Vec<JobRecord>,
    /// (total, clearable) storage bytes, computed when Settings opens.
    storage_size: Option<(u64, u64)>,
    settings_open: bool,
    /// Right-click context menu: the targeted entry and the cursor position.
    context: Option<(Entry, Point<Pixels>)>,
    /// Right-click on empty list space: the cursor position.
    bg_menu: Option<Point<Pixels>>,
    /// Whether the rcd status popover (status-bar daemon button) is open.
    rc_popover_open: bool,
    /// Open command palette (⌘⇧P).
    command_palette: Option<Entity<Picker<CommandPaletteDelegate>>>,
    command_palette_sub: Option<gpui::Subscription>,
    /// Pending confirmation modal (destructive or irreversible actions).
    confirm: Option<Entity<ConfirmModal>>,
    /// Subscription to the open confirm modal's accept/dismiss events.
    confirm_sub: Option<gpui::Subscription>,
    /// Pending text-input dialog (new folder, rename, …).
    prompt: Option<Entity<PromptModal>>,
    /// Subscription to the open prompt's submit/cancel events.
    prompt_sub: Option<gpui::Subscription>,
    /// Transient corner notifications (background-operation errors).
    toasts: Vec<Toast>,
    toast_seq: usize,
    jobs: Entity<Jobs>,
    jobs_open: bool,
    jobs_maximized: bool,
    /// Right-side file-preview pane.
    preview_open: bool,
    /// Preview of the cursor entry; rebuilt by `refresh_preview` as it moves.
    preview: Option<Preview>,
    /// Recently loaded previews, keyed by `remote:path` (LRU, bounded).
    preview_cache: Vec<(String, PreviewState)>,
    rc_health: RcHealth,
    clipboard: Option<Clipboard>,
    /// Whether the OS window is focused; toast dismiss timers pause when not
    /// (Sonner-style), so a toast can't expire while the user isn't looking.
    window_active: bool,
}

impl Workspace {
    fn new(
        service: Service,
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
        let sidebar_width = clamped_width(ui.sidebar_width, SIDEBAR_W, SIDEBAR_MIN, SIDEBAR_MAX);
        let preview_width = clamped_width(ui.preview_width, PREVIEW_W, PREVIEW_MIN, PREVIEW_MAX);
        let col_date_width = clamped_width(ui.col_date_width, COL_DATE, COL_MIN, COL_MAX);
        let col_size_width = clamped_width(ui.col_size_width, COL_SIZE, COL_MIN, COL_MAX);
        let jobs_maximized = ui.transfers_maximized;
        let preview_open = ui.preview_open;
        let jobs = cx.new(|_| Jobs::new(service.clone(), db.clone()));
        jobs.update(cx, |jobs, cx| jobs.start_polling(cx));
        cx.observe_window_activation(window, |this, window, _| {
            this.window_active = window.is_window_active();
        })
        .detach();
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
            version,
            focus,
            pane: Pane::Sidebar,
            remotes: Vec::new(),
            remote_sel: 0,
            remote_scroll: UniformListScrollHandle::new(),
            remote_menu: None,
            remote_config: None,
            remote_config_sub: None,
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
            history: Vec::new(),
            history_pos: 0,
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
            storage_size: None,
            settings_open: false,
            context: None,
            bg_menu: None,
            rc_popover_open: false,
            command_palette: None,
            command_palette_sub: None,
            confirm: None,
            confirm_sub: None,
            prompt: None,
            prompt_sub: None,
            toasts: Vec::new(),
            toast_seq: 0,
            jobs,
            jobs_open: false,
            jobs_maximized,
            preview_open,
            preview: None,
            preview_cache: Vec::new(),
            rc_health: RcHealth::Unknown,
            clipboard: None,
            window_active: true,
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

    fn load_remotes(&self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        cx.spawn(async move |this, cx| {
            let result = service.remotes().await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(remotes) => this.remotes = remotes,
                    Err(e) => this.toast(format!("Couldn't load remotes: {e}"), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn entries(&self) -> &[Entry] {
        self.dir_query.data().map(Vec::as_slice).unwrap_or(&[])
    }

    fn load_entries(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
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

    fn reload(&mut self, _: &Reload, _window: &mut Window, cx: &mut Context<Self>) {
        self.force_reload_entries(cx);
    }

    /// Force a refetch of the open directory, bypassing the stale gate.
    fn force_reload_entries(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let (field, order) = (self.sort_field, self.sort_order);
        self.dir_query.reload(cx, |this| &mut this.dir_query, move |(remote, path)| async move {
            let mut entries = service.list_dir(&remote, &path).await?;
            sort_entries(&mut entries, field, order);
            Ok::<_, ServiceError>(entries)
        });
    }

    /// Open the command palette, or close it if already open. Ignored while
    /// another modal is up (don't stack modals).
    fn toggle_palette(&mut self, _: &TogglePalette, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette.take().is_some() {
            cx.notify();
            return;
        }
        if self.confirm.is_some() || self.prompt.is_some() || self.remote_config.is_some() {
            return;
        }
        let previous_focus = window.focused(cx).unwrap_or_else(|| self.focus.clone());
        let workspace = cx.entity().downgrade();
        let service = self.service.clone();
        let db = self.db.clone();
        // Pinned-first (pin order preserved), matching the sidebar; the palette's
        // stable fuzzy sort keeps this order on empty query and score ties.
        let remotes = self.ordered_remotes();
        let current_remote = self.open_remote.clone();
        let palette = cx.new(|cx| {
            let delegate = CommandPaletteDelegate::new(
                previous_focus,
                workspace,
                service,
                db,
                remotes,
                current_remote,
                window,
            );
            Picker::new(delegate, window, cx)
        });
        self.command_palette_sub = Some(cx.subscribe(&palette, |this, _, _: &DismissEvent, cx| {
            this.command_palette = None;
            cx.notify();
        }));
        self.command_palette = Some(palette);
        cx.notify();
    }

    fn action_add_remote(&mut self, _: &AddRemote, _: &mut Window, cx: &mut Context<Self>) {
        self.begin_add_remote(cx);
    }

    fn action_open_settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.open_settings(cx);
    }

    /// Open Settings, computing the storage figures once (off the render path).
    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = true;
        self.refresh_storage_size();
        cx.notify();
    }

    /// (total, clearable) bytes on disk: the whole app dir, and just the cache.
    fn refresh_storage_size(&mut self) {
        self.storage_size = Some((dir_size(self.paths.root()), dir_size(&self.paths.cache_dir())));
    }

    fn action_restart_daemon(&mut self, _: &RestartDaemon, _: &mut Window, cx: &mut Context<Self>) {
        self.restart_daemon(cx);
    }

    fn action_toggle_transfers(&mut self, _: &ToggleTransfers, _: &mut Window, cx: &mut Context<Self>) {
        self.jobs_open = !self.jobs_open;
        cx.notify();
    }

    fn ask_confirm(
        &mut self,
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        confirm_label: impl Into<SharedString>,
        danger: bool,
        action: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let modal =
            cx.new(|cx| ConfirmModal::new(title, message, confirm_label, danger, cx));
        let mut action = Some(action);
        self.confirm_sub = Some(cx.subscribe(&modal, move |this, _, event, cx| {
            this.confirm = None;
            if let confirm::ConfirmEvent::Accepted = event {
                if let Some(action) = action.take() {
                    action(this, cx);
                }
            }
            cx.notify();
        }));
        self.confirm = Some(modal);
        cx.notify();
    }

    /// Start an inline edit; `action` runs with the entered text on submit.
    /// `target` is the renamed entry's path, or `None` for a new item at the top.
    fn begin_edit(
        &mut self,
        value: impl Into<String>,
        placeholder: impl Into<SharedString>,
        icon_dir: bool,
        target: Option<String>,
        action: impl FnOnce(&mut Self, String, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let modal =
            cx.new(|cx| PromptModal::new(value, placeholder, icon_dir, target, cx));
        let mut action = Some(action);
        self.prompt_sub = Some(cx.subscribe(&modal, move |this, _, event, cx| {
            match event {
                prompt::PromptEvent::Submitted(value) => {
                    this.prompt = None;
                    if let Some(action) = action.take() {
                        action(this, value.clone(), cx);
                    }
                }
                prompt::PromptEvent::Cancelled => this.prompt = None,
            }
            cx.notify();
        }));
        self.prompt = Some(modal);
        cx.notify();
    }

    fn choose_sort(&mut self, field: SortField, cx: &mut Context<Self>) {
        if self.sort_field == field {
            self.sort_order = self.sort_order.toggle();
        } else {
            self.sort_field = field;
        }
        let (field, order) = (self.sort_field, self.sort_order);
        self.store.update(|s| {
            s.sort_field = field;
            s.sort_order = order;
        });
        // Keep the selected item highlighted across the re-sort.
        self.pending_select = self.entries().get(self.entry_sel).map(|e| e.name.clone());
        self.dir_query.update_current(move |entries| sort_entries(entries, field, order));
        cx.notify();
    }

    fn load_remote(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(remote) = self.ordered_remotes().get(ix) {
            let name = remote.name.clone();
            self.navigate(name, String::new(), None, cx);
        }
    }

    fn is_pinned(&self, name: &str) -> bool {
        self.pinned.iter().any(|n| n == name)
    }

    /// Confirm, then remove a remote from the rclone config (files untouched).
    pub(crate) fn request_delete_remote(&mut self, name: String, cx: &mut Context<Self>) {
        let shown = name.clone();
        self.ask_confirm(
            "Delete remote?",
            format!(
                "Remove \u{201c}{shown}\u{201d} from the rclone config. Files on the remote are not deleted."
            ),
            "Delete",
            true,
            move |this, cx| this.delete_remote(name, cx),
            cx,
        );
    }

    fn delete_remote(&mut self, name: String, cx: &mut Context<Self>) {
        let service = self.service.clone();
        cx.spawn(async move |this, cx| {
            let result = service.config_delete(name.clone()).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        if this.open_remote.as_deref() == Some(name.as_str()) {
                            this.open_remote = None;
                            this.path = String::new();
                        }
                        this.pinned.retain(|n| n != &name);
                        this.db.save_pinned(&this.pinned);
                        this.load_remotes(cx);
                    }
                    Err(e) => this.toast(format!("Couldn't delete \"{name}\": {e}"), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Pinned remotes (in pinned order), then the rest in their existing sort.
    fn pinned_remotes(&self) -> Vec<RemoteInfo> {
        self.pinned
            .iter()
            .filter_map(|n| self.remotes.iter().find(|r| &r.name == n).cloned())
            .collect()
    }

    fn unpinned_remotes(&self) -> Vec<RemoteInfo> {
        self.remotes.iter().filter(|r| !self.is_pinned(&r.name)).cloned().collect()
    }

    fn ordered_remotes(&self) -> Vec<RemoteInfo> {
        let mut v = self.pinned_remotes();
        v.extend(self.unpinned_remotes());
        v
    }

    /// Pin or unpin `name`, keeping the keyboard selection on the same remote.
    fn toggle_pin(&mut self, name: String, cx: &mut Context<Self>) {
        let selected = self.ordered_remotes().get(self.remote_sel).map(|r| r.name.clone());
        match self.pinned.iter().position(|n| n == &name) {
            Some(pos) => {
                self.pinned.remove(pos);
            }
            None => self.pinned.push(name.clone()),
        }
        self.db.save_pinned(&self.pinned);
        self.select_remote(selected.as_deref());
        cx.notify();
    }

    /// Move pinned `from` to sit before pinned `before` (drop-to-reorder).
    fn reorder_pinned(&mut self, from: &str, before: &str, cx: &mut Context<Self>) {
        if from == before {
            return;
        }
        let selected = self.ordered_remotes().get(self.remote_sel).map(|r| r.name.clone());
        if let Some(fp) = self.pinned.iter().position(|n| n == from) {
            let name = self.pinned.remove(fp);
            let ip = self.pinned.iter().position(|n| n == before).unwrap_or(self.pinned.len());
            self.pinned.insert(ip, name);
            self.db.save_pinned(&self.pinned);
        }
        self.select_remote(selected.as_deref());
        cx.notify();
    }

    /// Shift a pinned remote one slot up or down within the pinned group.
    fn move_pinned(&mut self, name: &str, up: bool, cx: &mut Context<Self>) {
        let selected = self.ordered_remotes().get(self.remote_sel).map(|r| r.name.clone());
        if let Some(i) = self.pinned.iter().position(|n| n == name) {
            let j = if up { i.checked_sub(1) } else { (i + 1 < self.pinned.len()).then_some(i + 1) };
            if let Some(j) = j {
                self.pinned.swap(i, j);
                self.db.save_pinned(&self.pinned);
            }
        }
        self.select_remote(selected.as_deref());
        cx.notify();
    }

    /// Move the sidebar cursor/highlight onto `name` (no-op if it isn't listed).
    /// The highlight is derived from this by-name, so every path that opens or
    /// reorders remotes routes through here instead of poking `remote_sel`
    /// directly — the selection can't drift from the open remote.
    fn select_remote(&mut self, name: Option<&str>) {
        if let Some(name) = name {
            if let Some(ix) = self.ordered_remotes().iter().position(|r| r.name == name) {
                self.remote_sel = ix;
            }
        }
    }

    fn descend(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some((is_dir, path)) = self.entries().get(ix).map(|e| (e.is_dir, e.path.clone()))
        else {
            return;
        };
        if is_dir {
            let remote = self.open_remote.clone().unwrap_or_default();
            self.navigate(remote, path, None, cx);
        } else {
            // Opening a file selects it (so the preview has a target) and shows
            // it in the preview pane.
            self.select_only(ix);
            self.open_preview(cx);
        }
    }

    /// Close the open remote and return to the landing (welcome) view.
    fn go_home(&mut self, cx: &mut Context<Self>) {
        if self.open_remote.is_none() {
            return;
        }
        self.open_remote = None;
        self.path = String::new();
        self.entry_sel = 0;
        self.sel_anchor = 0;
        self.selected.clear();
        self.pending_select = None;
        self.preview = None;
        self.prompt = None;
        self.context = None;
        self.bg_menu = None;
        self.history.clear();
        self.history_pos = 0;
        self.pane = Pane::Sidebar;
        cx.notify();
    }

    /// Push a new location onto history, selecting `want` (by name) on arrival.
    /// Saves the current row first so going back restores it.
    fn navigate(&mut self, remote: String, path: String, want: Option<String>, cx: &mut Context<Self>) {
        // Record as recently-opened only when switching remotes, not per folder.
        if self.open_remote.as_deref() != Some(remote.as_str()) {
            self.db.record_remote(&remote);
            self.recent_remotes = self.db.recent_remotes(RECENT_REMOTES_FETCH);
        }
        // Keep the sidebar highlight on the remote being shown. Every open path
        // routes through navigate(), so syncing here covers all of them.
        self.select_remote(Some(&remote));
        self.remember_sel();
        self.open_remote = Some(remote.clone());
        self.path = path.clone();
        self.history.truncate(self.history_pos + 1);
        self.history.push(Location { remote, path, selected: None });
        self.history_pos = self.history.len() - 1;
        self.entry_sel = 0;
        self.sel_anchor = 0;
        self.selected.clear();
        self.pending_select = want;
        self.load_entries(cx);
    }

    /// Reveal a job target in the explorer: open a folder directly, or open a
    /// file's containing directory with the file selected.
    pub(crate) fn reveal_target(&mut self, target: JobTarget, cx: &mut Context<Self>) {
        self.jobs_open = false;
        self.pane = Pane::Explorer;
        if target.is_dir {
            self.navigate(target.remote, target.path, None, cx);
        } else {
            let containing_dir = parent_of(&target.path).to_string();
            self.navigate(target.remote, containing_dir, Some(target.name.to_string()), cx);
        }
    }

    /// Resize a file-list column by dragging its left divider. Width is measured
    /// from the table's right content edge (the Name column flex-grows to fill).
    fn on_column_drag(&mut self, e: &DragMoveEvent<DragColumn>, _: &mut Window, cx: &mut Context<Self>) {
        let x = f32::from(e.event.position.x);
        // Table content edge: body bounds minus the rows' horizontal padding.
        let right = f32::from(e.bounds.right()) - TABLE_PAD;
        // Column order is Name (flex), Size, Date — Date is flush right; Size is
        // flush to its left. Anchor each from the content edge so the dragged
        // divider tracks the cursor exactly.
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

    fn reset_column(&mut self, column: Column, cx: &mut Context<Self>) {
        match column {
            Column::Date => self.col_date_width = px(COL_DATE),
            Column::Size => self.col_size_width = px(COL_SIZE),
        }
        cx.notify();
    }

    /// Save any pane or column width a resize changed (called on mouse release).
    fn persist_pane_widths(&mut self, _: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        let (sidebar, preview, date, size) = (
            f32::from(self.sidebar_width),
            f32::from(self.preview_width),
            f32::from(self.col_date_width),
            f32::from(self.col_size_width),
        );
        let unchanged = (self.ui.sidebar_width, self.ui.preview_width, self.ui.col_date_width, self.ui.col_size_width)
            == (Some(sidebar), Some(preview), Some(date), Some(size));
        if !unchanged {
            self.ui.sidebar_width = Some(sidebar);
            self.ui.preview_width = Some(preview);
            self.ui.col_date_width = Some(date);
            self.ui.col_size_width = Some(size);
            self.save_ui();
        }
    }

    /// Persist the cached [`UiState`] to the database (best-effort).
    fn save_ui(&self) {
        self.db.save_ui(&self.ui);
    }

    fn remember_sel(&mut self) {
        let name = self.entries().get(self.entry_sel).map(|e| e.name.clone());
        if let Some(loc) = self.history.get_mut(self.history_pos) {
            loc.selected = name;
        }
    }

    fn can_back(&self) -> bool {
        self.history_pos > 0
    }

    fn can_forward(&self) -> bool {
        self.history_pos + 1 < self.history.len()
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.can_back() {
            self.remember_sel();
            self.history_pos -= 1;
            self.restore_history(cx);
        }
    }

    fn go_forward(&mut self, cx: &mut Context<Self>) {
        if self.can_forward() {
            self.remember_sel();
            self.history_pos += 1;
            self.restore_history(cx);
        }
    }

    fn restore_history(&mut self, cx: &mut Context<Self>) {
        let loc = self.history[self.history_pos].clone();
        self.open_remote = Some(loc.remote);
        self.path = loc.path;
        self.pane = Pane::Explorer;
        self.entry_sel = 0;
        self.sel_anchor = 0;
        self.selected.clear();
        self.pending_select = loc.selected;
        self.load_entries(cx);
    }

    /// Apply a pending select-by-name once its listing has loaded, then clamp.
    /// A freshly opened directory has *no* selection (Finder-style) — only an
    /// explicit `pending_select` (e.g. after rename, or the child folder when
    /// going up) selects an item.
    fn resolve_selection(&mut self) {
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
        let len = self.entries().len();
        if len > 0 && self.entry_sel >= len {
            self.entry_sel = len - 1;
        }
        // Drop any selected paths that no longer exist in the listing.
        if !self.selected.is_empty() {
            let valid: HashSet<String> = self.entries().iter().map(|e| e.path.clone()).collect();
            self.selected.retain(|p| valid.contains(p));
        }
    }

    fn minimize(&mut self, _: &Minimize, window: &mut Window, _cx: &mut Context<Self>) {
        window.minimize_window();
    }

    fn zoom(&mut self, _: &Zoom, window: &mut Window, _cx: &mut Context<Self>) {
        window.zoom_window();
    }

    fn toggle_fullscreen(&mut self, _: &ToggleFullscreen, window: &mut Window, _cx: &mut Context<Self>) {
        window.toggle_fullscreen();
    }

    fn close_settings(&mut self, _: &CloseSettings, _window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open
            || self.context.is_some()
            || self.remote_menu.is_some()
            || self.bg_menu.is_some()
            || self.command_palette.is_some()
            || self.confirm.is_some()
            || self.prompt.is_some()
            || self.remote_config.is_some()
            || self.jobs_open
        {
            self.settings_open = false;
            self.jobs_open = false;
            self.command_palette = None;
            self.confirm = None;
            self.prompt = None;
            self.close_remote_config(cx);
            self.close_menus();
            cx.notify();
        } else if self.pane == Pane::Explorer && self.selected.len() > 1 {
            // Nothing to close: collapse a multi-selection back to the cursor.
            self.select_only(self.entry_sel);
            cx.notify();
        }
    }

    fn set_refresh(&mut self, secs: u64, cx: &mut Context<Self>) {
        self.store.update(|s| s.refresh_secs = secs);
        self.dir_query.set_stale_after(Some(Duration::from_secs(secs.max(1))));
        cx.notify();
    }

    fn choose_download_dir(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(dir) = paths.into_iter().next() {
                    this.update(cx, |this, cx| {
                        this.store.update(|s| s.download_dir = Some(dir.to_string_lossy().into_owned()));
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn copy_to_clipboard(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Download every selected entry to the configured folder, one job each.
    fn action_back(&mut self, _: &GoBack, _window: &mut Window, cx: &mut Context<Self>) {
        self.go_back(cx);
    }

    fn action_forward(&mut self, _: &GoForward, _window: &mut Window, cx: &mut Context<Self>) {
        self.go_forward(cx);
    }

    fn active_len(&self) -> usize {
        match self.pane {
            Pane::Sidebar => self.remotes.len(),
            Pane::Explorer => self.entries().len(),
        }
    }

    /// Keep the selected row of the active pane in view (scrolls only when
    /// off-screen).
    fn scroll_to_selection(&self) {
        match self.pane {
            Pane::Sidebar => {
                self.remote_scroll.scroll_to_item(self.remote_sel, ScrollStrategy::Nearest)
            }
            Pane::Explorer => {
                self.entry_scroll.scroll_to_item(self.entry_sel, ScrollStrategy::Nearest)
            }
        }
    }

    fn entry_path_at(&self, ix: usize) -> Option<String> {
        self.entries().get(ix).map(|e| e.path.clone())
    }

    /// Selected entries in display order; falls back to the cursor row.
    fn selected_entries(&self) -> Vec<Entry> {
        // No selection means no operands — keyboard copy/cut/delete/download
        // no-op rather than silently acting on the cursor row.
        if self.selected.is_empty() {
            return Vec::new();
        }
        self.entries().iter().filter(|e| self.selected.contains(&e.path)).cloned().collect()
    }

    /// Cursor becomes the sole selection (plain click / arrow).
    fn select_only(&mut self, ix: usize) {
        self.entry_sel = ix;
        self.sel_anchor = ix;
        self.selected.clear();
        if let Some(p) = self.entry_path_at(ix) {
            self.selected.insert(p);
        }
    }

    /// Toggle `ix`'s membership (cmd-click); cursor and anchor move to it.
    fn toggle_at(&mut self, ix: usize) {
        self.entry_sel = ix;
        self.sel_anchor = ix;
        if let Some(p) = self.entry_path_at(ix) {
            if !self.selected.remove(&p) {
                self.selected.insert(p);
            }
        }
    }

    /// Select the inclusive anchor..=ix range (shift-click / shift-arrow).
    fn select_range_to(&mut self, ix: usize) {
        let (lo, hi) = (self.sel_anchor.min(ix), self.sel_anchor.max(ix));
        let paths: Vec<String> = self
            .entries()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= lo && *i <= hi)
            .map(|(_, e)| e.path.clone())
            .collect();
        self.selected = paths.into_iter().collect();
        self.entry_sel = ix;
    }

    fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pane != Pane::Explorer {
            return;
        }
        let all: HashSet<String> = self.entries().iter().map(|e| e.path.clone()).collect();
        self.selected = all;
        cx.notify();
    }

    fn select_next(&mut self, _: &SelectNext, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.active_len();
        if len == 0 {
            return;
        }
        match self.pane {
            Pane::Sidebar => {
                if self.remote_sel + 1 < len {
                    self.remote_sel += 1;
                }
            }
            Pane::Explorer => {
                // From no selection, the first Down selects the first row.
                if self.selected.is_empty() {
                    self.select_only(0);
                } else {
                    let next = (self.entry_sel + 1).min(len - 1);
                    if window.modifiers().shift {
                        self.select_range_to(next);
                    } else {
                        self.select_only(next);
                    }
                }
            }
        }
        cx.notify();
        self.scroll_to_selection();
    }

    fn select_prev(&mut self, _: &SelectPrev, window: &mut Window, cx: &mut Context<Self>) {
        match self.pane {
            Pane::Sidebar => self.remote_sel = self.remote_sel.saturating_sub(1),
            Pane::Explorer => {
                let len = self.entries().len();
                if len == 0 {
                    return;
                }
                // From no selection, the first Up selects the last row.
                if self.selected.is_empty() {
                    self.select_only(len - 1);
                    cx.notify();
                    self.scroll_to_selection();
                    return;
                }
                let prev = self.entry_sel.saturating_sub(1);
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

    fn open(&mut self, _: &Open, _window: &mut Window, cx: &mut Context<Self>) {
        match self.pane {
            Pane::Sidebar => {
                self.load_remote(self.remote_sel, cx);
                self.pane = Pane::Explorer;
            }
            Pane::Explorer => self.descend(self.entry_sel, cx),
        }
    }

    fn go_up(&mut self, _: &GoUp, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pane != Pane::Explorer {
            return;
        }
        if self.path.is_empty() {
            self.pane = Pane::Sidebar;
            cx.notify();
        } else {
            let child = self.path.rsplit('/').next().unwrap_or_default().to_string();
            let parent = parent_of(&self.path).to_string();
            let remote = self.open_remote.clone().unwrap_or_default();
            self.navigate(remote, parent, Some(child), cx);
        }
    }

    fn toggle_pane(&mut self, _: &TogglePane, _window: &mut Window, cx: &mut Context<Self>) {
        self.pane = match self.pane {
            Pane::Sidebar if self.open_remote.is_some() => Pane::Explorer,
            Pane::Sidebar => Pane::Sidebar,
            Pane::Explorer => Pane::Sidebar,
        };
        cx.notify();
    }

    fn focus_sidebar(&mut self, _: &FocusSidebar, _window: &mut Window, cx: &mut Context<Self>) {
        self.pane = Pane::Sidebar;
        cx.notify();
    }

    fn focus_explorer(&mut self, _: &FocusExplorer, _window: &mut Window, cx: &mut Context<Self>) {
        if self.open_remote.is_some() {
            self.pane = Pane::Explorer;
            cx.notify();
        }
    }

    fn active_remote(&self) -> Option<&RemoteInfo> {
        let name = self.open_remote.as_ref()?;
        self.remotes.iter().find(|r| &r.name == name)
    }

    fn copy_text(&self) -> String {
        match &self.open_remote {
            Some(r) => format!("{r}:{}", self.path),
            None => String::new(),
        }
    }

    /// A copy-to-clipboard button: copy/check icon, tooltip, and a check-flash
    /// scoped to `source`. Shared by the breadcrumb, error card, and task rows.
    fn copy_button(
        &self,
        id: impl Into<gpui::ElementId>,
        source: CopySource,
        text: String,
        tip: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let done = self.copied == Some(source);
        h_flex()
            .id(id)
            .size(px(22.0))
            .flex_shrink_0()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .hover(|s| s.bg(rgba(OVERLAY)))
            .tooltip(tooltip_text(if done { "Copied" } else { tip }))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.copy_with_feedback(source, text.clone(), cx)
            }))
            .child(
                svg()
                    .path(if done { "icons/check.svg" } else { "icons/copy.svg" })
                    .size(px(13.0))
                    .text_color(rgb(if done { SUCCESS } else { FG_MUTED })),
            )
    }

    /// Copy `text` and flash the check on `source`'s button for 1.2s.
    fn copy_with_feedback(&mut self, source: CopySource, text: String, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = Some(source);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1200)).await;
            this.update(cx, |this, cx| {
                if this.copied == Some(source) {
                    this.copied = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.resolve_selection();
        self.refresh_preview(cx);
        // Keep focus on the open dialog, else on the workspace — so each owns the
        // keyboard while shown, and focus returns here when it closes. The modal
        // entities (remote config, confirm) steer their own focus.
        if self.remote_config.is_some()
            || self.confirm.is_some()
            || self.prompt.is_some()
            || self.command_palette.is_some()
        {
            // modal entities own their focus
        } else if !self.focus.is_focused(window) {
            self.focus.focus(window, cx);
        }
        v_flex()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::go_up))
            .on_action(cx.listener(Self::action_back))
            .on_action(cx.listener(Self::action_forward))
            .on_action(cx.listener(Self::reload))
            .on_action(cx.listener(Self::minimize))
            .on_action(cx.listener(Self::zoom))
            .on_action(cx.listener(Self::toggle_fullscreen))
            .on_action(cx.listener(Self::close_settings))
            .on_action(cx.listener(Self::toggle_pane))
            .on_action(cx.listener(Self::focus_sidebar))
            .on_action(cx.listener(Self::focus_explorer))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::new_folder))
            .on_action(cx.listener(Self::new_file))
            .on_action(cx.listener(Self::rename))
            .on_action(cx.listener(Self::toggle_preview))
            .on_action(cx.listener(Self::toggle_palette))
            .on_action(cx.listener(Self::action_add_remote))
            .on_action(cx.listener(Self::action_open_settings))
            .on_action(cx.listener(Self::action_restart_daemon))
            .on_action(cx.listener(Self::action_toggle_transfers))
            .on_drag_move(cx.listener(|this, e: &DragMoveEvent<DragResize>, window, cx| {
                let x = f32::from(e.event.position.x);
                let (width, current) = match e.drag(cx).0 {
                    ResizeTarget::Sidebar => {
                        (px(x.clamp(SIDEBAR_MIN, SIDEBAR_MAX)), &mut this.sidebar_width)
                    }
                    ResizeTarget::Preview => {
                        // Pane is docked right: width grows as the cursor nears the edge.
                        let from_right = f32::from(window.viewport_size().width) - x;
                        (px(from_right.clamp(PREVIEW_MIN, PREVIEW_MAX)), &mut this.preview_width)
                    }
                };
                if width != *current {
                    *current = width;
                    cx.notify();
                }
            }))
            // Persist pane widths once the resize drag releases, not per move.
            .on_mouse_up(MouseButton::Left, cx.listener(Self::persist_pane_widths))
            .size_full()
            .bg(rgb(CANVAS))
            .text_color(rgb(FG))
            .text_sm()
            .child(self.render_title_bar(window, cx))
            .child({
                // A panel covers the browser only while open AND zoomed, so
                // closing it can never leave the content region blank.
                let zoomed = self.jobs_open && self.jobs_maximized;
                v_flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .when(!zoomed, |el| {
                        el.child(
                            // Plain flex_row, not h_flex: panes stretch to full height.
                            div()
                                .flex()
                                .flex_row()
                                .flex_1()
                                .min_h(px(0.0))
                                .w_full()
                                .child(self.render_sidebar(cx))
                                .child(self.render_explorer(cx))
                                .when(self.preview_open, |el| el.child(self.render_preview(cx))),
                        )
                    })
                    .when(self.jobs_open, |el| el.child(self.render_transfers(cx)))
            })
            .child(self.render_status_bar(cx))
            .when(self.context.is_some(), |el| el.child(self.render_context_menu(cx)))
            .when(self.remote_menu.is_some(), |el| el.child(self.render_remote_menu(cx)))
            .when(self.bg_menu.is_some(), |el| el.child(self.render_bg_menu(cx)))
            .when(self.rc_popover_open, |el| el.child(self.rc_popover_backdrop(cx)))
            .when_some(self.command_palette.clone(), |el, palette| {
                el.child(self.modal_overlay(
                    true,
                    true,
                    |this, cx| {
                        this.command_palette = None;
                        cx.notify();
                    },
                    palette,
                    cx,
                ))
            })
            .when_some(self.confirm.clone(), |el, modal| {
                el.child(self.modal_overlay(
                    true,
                    false,
                    |this, cx| {
                        this.confirm = None;
                        cx.notify();
                    },
                    modal,
                    cx,
                ))
            })
            .when_some(self.remote_config.clone(), |el, modal| {
                el.child(self.modal_overlay(
                    false,
                    false,
                    |this, cx| this.close_remote_config(cx),
                    modal,
                    cx,
                ))
            })
            .when(self.settings_open, |el| el.child(self.render_settings(cx)))
            .when(!self.toasts.is_empty(), |el| el.child(self.render_toasts(cx)))
    }
}

