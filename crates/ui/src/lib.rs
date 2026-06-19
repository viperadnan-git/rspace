//! gpui desktop shell: a two-pane remote browser.

mod command_palette;
mod confirm;
mod fuzzy;
mod jobs;
mod menus;
mod mount_options;
mod number_field;
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
use toast::{Toast, ToastBody};
use transfers::{Job, JobTarget, Jobs, JobsEvent};
use remotes::RemoteConfigModal;
use mount_options::MountOptionsModal;
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
            "plus", "server_network", "server_network_off", "github", "search", "corner_down_left"
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
                let (rclone_bin, version) = match &rclone {
                    RcloneStatus::Found { path, version } => (path.clone(), version.clone()),
                    _ => (String::new(), String::new()),
                };
                cx.open_window(options, |window, cx| {
                    cx.new(|cx| Workspace::new(service, rclone_bin, version, paths, store, db, window, cx))
                })
                .unwrap();
            }
            None => {
                cx.open_window(options, |_, cx| cx.new(|cx| StatusScreen::new(rclone, store, cx)))
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
        KeyBinding::new("down", SelectNext, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("j", SelectNext, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("up", SelectPrev, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("k", SelectPrev, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("shift-down", SelectNext, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("shift-j", SelectNext, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("shift-up", SelectPrev, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("shift-k", SelectPrev, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-a", SelectAll, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("enter", Open, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("tab", TogglePane, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("backspace", GoUp, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-[", GoBack, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-]", GoForward, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-r", Reload, Some("Workspace && !modal && !TextInput")),
        // Toggle (not !modal) so it can also close itself; the handler ignores
        // it while another modal is open.
        // The modern "cmdk" command-menu shortcut: cmd-k on macOS, ctrl-k elsewhere.
        KeyBinding::new("secondary-k", TogglePalette, Some("Workspace")),
        KeyBinding::new("left", FocusSidebar, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("right", FocusExplorer, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-c", CopyEntry, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-x", CutEntry, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-v", PasteEntry, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-backspace", DeleteEntry, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-shift-n", NewFolder, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-u", NewFile, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("f2", Rename, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("space", TogglePreview, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("escape", CloseSettings, Some("Workspace")),
        // Add/edit-remote dialog: arrows (or ctrl-n/p) navigate the picker,
        // Enter advances. Bound to its own context so any focusable list can reuse.
        KeyBinding::new("down", ConfigNext, Some("RemoteConfig")),
        KeyBinding::new("ctrl-n", ConfigNext, Some("RemoteConfig")),
        KeyBinding::new("up", ConfigPrev, Some("RemoteConfig")),
        KeyBinding::new("ctrl-p", ConfigPrev, Some("RemoteConfig")),
        // Enter confirms the current step from anywhere in the modal (matching the
        // other dialogs), so blurring a field doesn't disable it.
        KeyBinding::new("enter", ConfigConfirm, Some("RemoteConfig")),
        KeyBinding::new("tab", FocusNext, Some("RemoteConfig")),
        KeyBinding::new("shift-tab", FocusPrev, Some("RemoteConfig")),
        KeyBinding::new("enter", ConfirmAccept, Some("Confirm")),
        KeyBinding::new("enter", PromptSubmit, Some("Prompt")),
        KeyBinding::new("escape", PromptCancel, Some("Prompt")),
        KeyBinding::new("enter", MountSave, Some("MountOptions")),
        KeyBinding::new("escape", MountCancel, Some("MountOptions")),
        KeyBinding::new("enter", SetupSubmit, Some("Setup")),
        KeyBinding::new("enter", NumberCommit, Some("NumberField")),
        KeyBinding::new("enter", SearchSubmit, Some("ExplorerSearch")),
        // Toggle works while the search field is focused too, so it can close it.
        KeyBinding::new("secondary-f", ToggleSearch, Some("Workspace && !modal")),
        KeyBinding::new("escape", CloseSearch, Some("ExplorerSearch")),
    ]);
    // Minimize is a macOS app convention (cmd-m); elsewhere the window manager owns it.
    #[cfg(target_os = "macos")]
    cx.bind_keys([KeyBinding::new("cmd-m", Minimize, None)]);
}

const REPO_URL: &str = "https://github.com/viperadnan-git/rspace";

/// Re-exec the app so a freshly-saved rclone path takes effect from a clean
/// start (avoids transitioning the daemon/window in place).
fn relaunch() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(std::env::args_os().skip(1));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let _ = cmd.exec(); // replaces this process on success
    }
    #[cfg(not(unix))]
    {
        let _ = cmd.spawn();
        std::process::exit(0);
    }
}

/// Pre-workspace setup screen, shown when no usable rclone is configured: the
/// brand, an install link, a field to point rspace at an rclone binary, and a
/// link to the project.
struct StatusScreen {
    rclone: RcloneStatus,
    store: SettingsStore,
    path_input: Entity<TextInput>,
    focus_handle: FocusHandle,
    error: Option<SharedString>,
    /// The manual-path form is hidden until the user opts into it.
    show_path: bool,
    /// Focus the screen once on open (not every frame, which would steal focus
    /// back from the path input on click).
    focused: bool,
}

impl Focusable for StatusScreen {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl StatusScreen {
    fn new(rclone: RcloneStatus, store: SettingsStore, cx: &mut Context<Self>) -> Self {
        let placeholder =
            if cfg!(windows) { "C:\\path\\to\\rclone.exe" } else { "/usr/local/bin/rclone" };
        let path_input = cx.new(|cx| TextInput::new(cx, placeholder));
        if let Some(p) = store.get().rclone_path.clone() {
            path_input.update(cx, |i, cx| i.set_text(p, cx));
        }
        Self {
            rclone,
            store,
            path_input,
            focus_handle: cx.focus_handle(),
            error: None,
            show_path: false,
            focused: false,
        }
    }

    fn submit(&mut self, _: &SetupSubmit, _: &mut Window, cx: &mut Context<Self>) {
        self.do_submit(cx);
    }

    fn do_submit(&mut self, cx: &mut Context<Self>) {
        let path = self.path_input.read(cx).text().trim().to_string();
        if path.is_empty() {
            self.error = Some("Enter the path to the rclone binary".into());
            cx.notify();
            return;
        }
        if rspace_rclone_rc::from_path(&path).is_none() {
            self.error = Some(format!("No working rclone at \u{201c}{path}\u{201d}").into());
            cx.notify();
            return;
        }
        self.store.update(|s| s.rclone_path = Some(path));
        relaunch();
    }

    /// Re-run detection (e.g. after the user installed rclone); relaunch on
    /// success so it starts cleanly, else surface that it's still missing.
    fn check_again(&mut self, cx: &mut Context<Self>) {
        if rspace_rclone_rc::detect().is_ok() {
            relaunch();
        } else {
            self.error = Some("rclone still isn't detected — install it, or enter its path below.".into());
            cx.notify();
        }
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        pick_file_into(self.path_input.clone(), cx);
    }

}

impl Render for StatusScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        focus_once(&mut self.focused, &self.focus_handle, window, cx);
        let missing = matches!(self.rclone, RcloneStatus::Missing { .. });
        let (heading, sub): (SharedString, SharedString) = match &self.rclone {
            RcloneStatus::Error { message } => ("rclone won't start".into(), message.clone().into()),
            _ => (
                "Set up rclone".into(),
                "rspace uses the rclone binary to reach your cloud storage.".into(),
            ),
        };

        let header = v_flex()
            .items_center()
            .gap_1p5()
            .child(
                div()
                    .text_size(px(21.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(FG))
                    .child(heading),
            )
            .child(div().max_w(px(300.0)).text_sm().text_center().text_color(rgb(FG_MUTED)).child(sub));

        let install_url = rspace_rclone_rc::INSTALL_URL.to_string();
        let actions = v_flex()
            .w_full()
            .max_w(px(340.0))
            .items_center()
            .gap_4()
            .when(missing, |el| {
                el.child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(Button::new("setup-install", "Install rclone", ButtonStyle::Primary).build(
                            move |_, cx| cx.open_url(&install_url),
                            cx,
                        ))
                        .child(Button::new("setup-recheck", "Check again", ButtonStyle::Soft).build(
                            |this, cx| this.check_again(cx),
                            cx,
                        )),
                )
            })
            // Separate the install actions from the manual-path option.
            .when(missing, |el| el.child(divider()))
            // The manual-path form stays hidden behind a quiet link to keep the
            // first impression minimal.
            .when(!self.show_path, |el| {
                el.child(text_link("setup-reveal-path", "Enter rclone path manually", None, |this, window, cx| {
                    this.show_path = true;
                    this.path_input.read(cx).focus_handle(cx).focus(window, cx);
                    cx.notify();
                }, cx))
            })
            .when(self.show_path, |el| {
                el.child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .child(div().flex_1().min_w(px(0.0)).child(self.path_input.clone()))
                                .child(Button::new("setup-browse", "Browse\u{2026}", ButtonStyle::Soft).build(|this, cx| {
                                    this.browse(cx)
                                }, cx)),
                        )
                        .when_some(self.error.clone(), |el, e| {
                            el.child(div().text_xs().text_color(rgb(DANGER)).child(e))
                        })
                        .child(
                            h_flex().w_full().justify_center().child(
                                Button::new("setup-save", "Use this path", ButtonStyle::Primary)
                                    .build(|this, cx| this.do_submit(cx), cx),
                            ),
                        ),
                )
            });

        let footer = h_flex().w_full().justify_center().pb_8().child(
            text_link("gh-link", "viperadnan-git/rspace", Some("icons/github.svg"), |_, _, cx| {
                cx.open_url(REPO_URL)
            }, cx)
            .tooltip(tooltip_text(REPO_URL)),
        );

        v_flex()
            .size_full()
            .bg(rgb(INSET))
            .text_color(rgb(FG))
            .key_context("Setup")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::submit))
            .child(
                v_flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .items_center()
                    .justify_center()
                    .gap_7()
                    .p_8()
                    .child(brand_mark())
                    .child(header)
                    .child(actions),
            )
            .child(footer)
    }
}

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
    /// Open add/edit-remote modal (schema-driven, backend-agnostic).
    remote_config: Option<Entity<RemoteConfigModal>>,
    /// Subscription to the open modal's dismiss/saved events.
    remote_config_sub: Option<gpui::Subscription>,
    /// Open per-remote mount-options modal + its event subscription.
    mount_options: Option<Entity<MountOptionsModal>>,
    mount_options_sub: Option<gpui::Subscription>,
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
    command_palette: Option<Entity<Picker<CommandPaletteDelegate>>>,
    command_palette_sub: Option<gpui::Subscription>,
    confirm: Option<Entity<ConfirmModal>>,
    confirm_sub: Option<gpui::Subscription>,
    prompt: Option<Entity<PromptModal>>,
    prompt_sub: Option<gpui::Subscription>,
    toasts: Vec<Toast>,
    toast_seq: usize,
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
    /// Whether the OS window is focused; toast dismiss timers pause when not
    /// (Sonner-style), so a toast can't expire while the user isn't looking.
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
            rclone_bin,
            version,
            focus,
            pane: Pane::Sidebar,
            remotes: Vec::new(),
            remote_sel: 0,
            remote_scroll: UniformListScrollHandle::new(),
            remote_menu: None,
            remote_config: None,
            remote_config_sub: None,
            mount_options: None,
            rclone_edit: None,
            rclone_edit_focus: false,
            mount_options_sub: None,
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
            command_palette: None,
            command_palette_sub: None,
            confirm: None,
            confirm_sub: None,
            prompt: None,
            prompt_sub: None,
            toasts: Vec::new(),
            toast_seq: 0,
            jobs,
            refresh_field,
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
        if self.recursive_showing() {
            self.search_query.data().map(Vec::as_slice).unwrap_or(&[])
        } else if self.has_query() {
            &self.view
        } else {
            self.dir_query.data().map(Vec::as_slice).unwrap_or(&[])
        }
    }

    fn has_query(&self) -> bool {
        self.search.split_whitespace().next().is_some()
    }

    fn recursive_intent(&self) -> bool {
        self.searched.as_deref() == Some(self.search.as_str())
    }

    fn recursive_showing(&self) -> bool {
        self.recursive_intent() && self.search_query.data().is_some()
    }

    /// Per-frame; skips rebuild when query and dir entries are unchanged.
    fn rebuild_search_view(&mut self) {
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

    fn search_submit(&mut self, _: &SearchSubmit, _: &mut Window, cx: &mut Context<Self>) {
        self.run_search(cx);
    }

    fn toggle_subfolder_search(&mut self, cx: &mut Context<Self>) {
        if self.recursive_intent() {
            self.searched = None;
            cx.notify();
        } else {
            self.run_search(cx);
        }
    }

    fn run_search(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
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

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = !self.search_open;
        if self.search_open {
            self.search_input.read(cx).focus_handle(cx).focus(window, cx);
        } else {
            self.reset_search(cx);
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    fn toggle_search_action(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_search(window, cx);
    }

    fn close_search(&mut self, _: &CloseSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open {
            self.search_open = false;
            self.reset_search(cx);
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    fn clear_search(&mut self, cx: &mut Context<Self>) {
        self.searched = None;
        self.search.clear();
        self.search_input.update(cx, |i, cx| i.set_text(String::new(), cx));
        cx.notify();
    }

    fn reset_search(&mut self, cx: &mut Context<Self>) {
        self.search_open = false;
        self.searched = None;
        self.view_sig = None;
        if !self.search.is_empty() {
            self.search.clear();
            self.search_input.update(cx, |i, cx| i.set_text(String::new(), cx));
        }
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
        if self.confirm.is_some()
            || self.prompt.is_some()
            || self.remote_config.is_some()
            || self.mount_options.is_some()
        {
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

    fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = true;
        self.refresh_storage_size();
        self.fetch_rclone_info(cx);
        cx.notify();
    }

    fn refresh_storage_size(&mut self) {
        self.storage_size = Some((dir_size(self.paths.root()), dir_size(&self.paths.cache_dir())));
    }

    /// Resolve rclone's own paths (`config/paths`, fetched once) and size its
    /// cache. The VFS cache can be many GB, so the walk runs on the background
    /// executor rather than blocking the UI thread.
    fn fetch_rclone_info(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        // Resolve paths only once (they don't change at runtime); the size walk
        // runs every open so it stays fresh.
        let cache = self.rclone_paths.as_ref().map(|p| p.cache.clone());
        cx.spawn(async move |this, cx| {
            let (cache, fetched) = match cache {
                Some(cache) => (cache, None),
                None => match service.config_paths().await {
                    Ok(paths) => (paths.cache.clone(), Some(paths)),
                    Err(_) => return,
                },
            };
            let size = cx.background_executor().spawn(async move { dir_size(Path::new(&cache)) }).await;
            this.update(cx, |this, cx| {
                this.rclone_cache_size = Some(size);
                if let Some(paths) = fetched {
                    this.rclone_paths = Some(paths);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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
            self.select_only(ix);
            self.open_preview(cx);
        }
    }

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
        self.reset_search(cx);
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
            || self.mount_options.is_some()
            || self.jobs_open
        {
            self.settings_open = false;
            self.jobs_open = false;
            self.command_palette = None;
            self.confirm = None;
            self.prompt = None;
            self.mount_options = None;
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

    fn selected_entries(&self) -> Vec<Entry> {
        // No selection means no operands — keyboard copy/cut/delete/download
        // no-op rather than silently acting on the cursor row.
        if self.selected.is_empty() {
            return Vec::new();
        }
        self.entries().iter().filter(|e| self.selected.contains(&e.path)).cloned().collect()
    }

    fn select_only(&mut self, ix: usize) {
        self.entry_sel = ix;
        self.sel_anchor = ix;
        self.selected.clear();
        if let Some(p) = self.entry_path_at(ix) {
            self.selected.insert(p);
        }
    }

    fn toggle_at(&mut self, ix: usize) {
        self.entry_sel = ix;
        self.sel_anchor = ix;
        if let Some(p) = self.entry_path_at(ix) {
            if !self.selected.remove(&p) {
                self.selected.insert(p);
            }
        }
    }

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
        self.rebuild_search_view();
        self.resolve_selection();
        self.refresh_preview(cx);
        // Keep focus on the open dialog, else on the workspace — so each owns the
        // keyboard while shown, and focus returns here when it closes. The modal
        // entities (remote config, confirm) steer their own focus.
        if self.remote_config.is_some()
            || self.confirm.is_some()
            || self.prompt.is_some()
            || self.command_palette.is_some()
            || self.mount_options.is_some()
        {
        } else if self.settings_open {
            // Settings inputs own their focus; focus a freshly-opened rclone edit
            // input once, then leave it be (re-focusing each frame would trap it).
            if let Some((_, input)) = self.rclone_edit.clone() {
                let handle = input.read(cx).focus_handle(cx);
                focus_once(&mut self.rclone_edit_focus, &handle, window, cx);
            }
        } else if self.search_input.read(cx).focus_handle(cx).is_focused(window) {
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
            .on_action(cx.listener(Self::toggle_search_action))
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
                            div()
                                .flex()
                                .flex_row()
                                .flex_1()
                                .min_h(px(0.0))
                                .w_full()
                                .child(self.render_sidebar(cx))
                                .child(self.render_explorer(cx)),
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
            .when_some(self.remote_config.clone(), |el, modal| {
                el.child(self.modal_overlay(
                    false,
                    false,
                    |this, cx| this.close_remote_config(cx),
                    modal,
                    cx,
                ))
            })
            .when_some(self.mount_options.clone(), |el, modal| {
                el.child(self.modal_overlay(
                    false,
                    false,
                    |this, cx| {
                        this.mount_options = None;
                        cx.notify();
                    },
                    modal,
                    cx,
                ))
            })
            .when(self.settings_open, |el| el.child(self.render_settings(cx)))
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
            .when(!self.toasts.is_empty(), |el| el.child(self.render_toasts(cx)))
    }
}

