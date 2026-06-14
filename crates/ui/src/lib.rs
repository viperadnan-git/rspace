//! gpui desktop shell: a two-pane remote browser.

mod query;

use std::ops::Range;
use std::time::Duration;

use gpui::{
    actions, anchored, deferred, div, point, prelude::*, px, relative, rgb, rgba, size, svg,
    uniform_list, Anchor,
    Animation, AnimationExt as _, AnyElement, AnyView, App, AssetSource, Bounds, ClickEvent, ClipboardItem,
    Context, Div,
    DragMoveEvent, FocusHandle, KeyBinding, Menu, MenuItem, MouseButton, MouseDownEvent,
    PathPromptOptions, Pixels, Point, ScrollStrategy, SharedString, Stateful, TitlebarOptions,
    UniformListScrollHandle, Window, WindowBounds, WindowOptions,
};
use gpui_platform::application;
use rspace_core::{Paths, SettingsStore, SortField, SortOrder};
use rspace_rclone_rc::{Entry, RemoteInfo, Service, ServiceError};

use query::{Query, Status};

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
        CloseSettings
    ]
);

// GitHub Dark Dimmed (Zed theme).
const CANVAS: u32 = 0x212830;
const INSET: u32 = 0x151b23;
const ELEVATED: u32 = 0x2a313c;
const BORDER_MUTED: u32 = 0x3d444d;
const FG: u32 = 0xd1d7e0;
const FG_MUTED: u32 = 0x9198a1;
const FG_SUBTLE: u32 = 0x656c76;
const ACCENT: u32 = 0x478be6;
const SUCCESS: u32 = 0x57ab5a;
const DANGER: u32 = 0xe5534b;
// Neutral element overlays (rgba) so they read over both pane backgrounds.
const OVERLAY: u32 = 0x656c7626;
const SELECT: u32 = 0x656c7659;
const SELECT_MUTED: u32 = 0x656c7633;

const SIDEBAR_W: f32 = 248.0;
const SIDEBAR_MIN: f32 = 180.0;
const SIDEBAR_MAX: f32 = 480.0;
const RESIZE_HANDLE_W: f32 = 6.0;
const TITLE_BAR_H: f32 = 36.0;
const MAX_CRUMBS: usize = 4;

/// Left inset of the custom title bar to clear the window controls.
#[cfg(target_os = "macos")]
const TITLE_BAR_LEAD: f32 = 80.0; // macOS traffic lights
#[cfg(not(target_os = "macos"))]
const TITLE_BAR_LEAD: f32 = 12.0;

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        Ok(match path {
            "icons/folder.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/folder.svg").as_slice()))
            }
            "icons/file.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/file.svg").as_slice()))
            }
            "icons/copy.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/copy.svg").as_slice()))
            }
            "icons/check.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/check.svg").as_slice()))
            }
            "icons/settings.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/settings.svg").as_slice()))
            }
            "icons/alert.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/alert.svg").as_slice()))
            }
            "icons/maximize.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/maximize.svg").as_slice()))
            }
            "icons/minimize.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/minimize.svg").as_slice()))
            }
            "icons/download.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/download.svg").as_slice()))
            }
            "icons/folder_open.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/folder_open.svg").as_slice()))
            }
            "icons/pin.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/pin.svg").as_slice()))
            }
            "icons/chevron_up.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/chevron_up.svg").as_slice()))
            }
            "icons/chevron_down.svg" => {
                Some(std::borrow::Cow::Borrowed(include_bytes!("../assets/icons/chevron_down.svg").as_slice()))
            }
            _ => None,
        })
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

/// A small hover tooltip (Zed-style: elevated box, used via [`tooltip_text`]).
struct Tooltip {
    text: SharedString,
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(ELEVATED))
            .border_1()
            .border_color(rgb(BORDER_MUTED))
            .shadow_lg()
            .text_xs()
            .text_color(rgb(FG))
            .child(self.text.clone())
    }
}

fn tooltip_text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let text = text.into();
    move |_window, cx| cx.new(|_| Tooltip { text: text.clone() }).into()
}

fn file_icon(is_dir: bool) -> impl IntoElement {
    let path = if is_dir { "icons/folder.svg" } else { "icons/file.svg" };
    svg().path(path).size(px(15.0)).flex_shrink_0().text_color(rgb(FG_MUTED))
}

fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

fn v_flex() -> Div {
    div().flex().flex_col()
}

fn list_item(id: usize, selected: bool, focused: bool) -> Stateful<Div> {
    let base = h_flex()
        .id(id)
        .w_full()
        .justify_between()
        .gap_2()
        .px_3()
        .py_1()
        .cursor_pointer();
    if selected && focused {
        base.bg(rgba(SELECT))
    } else if selected {
        base.bg(rgba(SELECT_MUTED))
    } else {
        base.hover(|s| s.bg(rgba(OVERLAY)))
    }
}

/// A square 22px icon button: muted svg glyph, rounded hover background.
fn icon_button(id: &'static str, icon: &'static str) -> Stateful<Div> {
    h_flex()
        .id(id)
        .size(px(22.0))
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .text_color(rgb(FG_MUTED))
        .hover(|s| s.bg(rgba(OVERLAY)))
        .child(svg().path(icon).size(px(14.0)).text_color(rgb(FG_MUTED)))
}

fn nav_button(id: &'static str, glyph: &'static str, enabled: bool) -> Stateful<Div> {
    let b = h_flex()
        .id(id)
        .size(px(24.0))
        .justify_center()
        .rounded_md()
        .text_color(if enabled { rgb(FG) } else { rgb(FG_SUBTLE) })
        .child(glyph);
    if enabled {
        b.cursor_pointer().hover(|s| s.bg(rgba(OVERLAY)))
    } else {
        b
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
}

/// Launch the desktop shell. Blocks until the app exits.
pub fn run(startup: Startup) {
    application().with_assets(Assets).run(move |cx: &mut App| {
        bind_keys(cx);
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

        let Startup { rclone, service, paths, store } = startup;
        match service {
            Some(service) => {
                let version = match &rclone {
                    RcloneStatus::Found { version, .. } => version.clone(),
                    _ => String::new(),
                };
                cx.open_window(options, |window, cx| {
                    cx.new(|cx| Workspace::new(service, version, paths, store, window, cx))
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
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullscreen, None),
        KeyBinding::new("down", SelectNext, Some("Workspace")),
        KeyBinding::new("j", SelectNext, Some("Workspace")),
        KeyBinding::new("up", SelectPrev, Some("Workspace")),
        KeyBinding::new("k", SelectPrev, Some("Workspace")),
        KeyBinding::new("enter", Open, Some("Workspace")),
        KeyBinding::new("tab", TogglePane, Some("Workspace")),
        KeyBinding::new("backspace", GoUp, Some("Workspace")),
        KeyBinding::new("cmd-[", GoBack, Some("Workspace")),
        KeyBinding::new("cmd-]", GoForward, Some("Workspace")),
        KeyBinding::new("cmd-r", Reload, Some("Workspace")),
        KeyBinding::new("escape", CloseSettings, Some("Workspace")),
        KeyBinding::new("left", FocusSidebar, Some("Workspace")),
        KeyBinding::new("right", FocusExplorer, Some("Workspace")),
    ]);
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
}

/// Reachability of the rclone rc daemon, surfaced by the status-bar dot.
#[derive(Clone)]
enum RcHealth {
    Unknown,
    Up,
    Down(String),
}

fn sort_arrow(order: SortOrder) -> &'static str {
    match order {
        SortOrder::Asc => "↑",
        SortOrder::Desc => "↓",
    }
}

#[derive(Clone)]
struct Location {
    remote: String,
    path: String,
    /// Name of the row selected here, restored by identity on return.
    selected: Option<String>,
}

/// A tracked rclone job (download/copy/…). State mirrors rclone's job + stats.
#[derive(Clone)]
struct Job {
    id: usize,
    group: String,
    jobid: Option<u64>,
    title: String,
    done: bool,
    error: Option<String>,
    bytes: u64,
    total: u64,
    speed: f64,
}

#[derive(Clone)]
struct DragSidebar;

impl Render for DragSidebar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// A pinned remote being dragged to reorder; the name identifies the source.
struct DraggedRemote {
    name: String,
}

/// The floating label rendered under the cursor while dragging a pinned remote.
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
            .border_1()
            .border_color(rgb(ACCENT))
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
    sidebar_width: Pixels,
    open_remote: Option<String>,
    /// Empty = root.
    path: String,
    entry_sel: usize,
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
    sort_menu_open: bool,
    paths: Paths,
    store: SettingsStore,
    settings_open: bool,
    /// Right-click context menu: the targeted entry and the cursor position.
    context: Option<(Entry, Point<Pixels>)>,
    jobs: Vec<Job>,
    job_seq: usize,
    jobs_open: bool,
    jobs_maximized: bool,
    rc_health: RcHealth,
}

impl Workspace {
    fn new(
        service: Service,
        version: String,
        paths: Paths,
        store: SettingsStore,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let stale = Duration::from_secs(store.get().refresh_secs.max(1));
        let (sort_field, sort_order) = (store.get().sort_field, store.get().sort_order);
        let this = Self {
            service,
            version,
            focus,
            pane: Pane::Sidebar,
            remotes: Vec::new(),
            remote_sel: 0,
            remote_scroll: UniformListScrollHandle::new(),
            remote_menu: None,
            sidebar_width: px(SIDEBAR_W),
            open_remote: None,
            path: String::new(),
            entry_sel: 0,
            entry_scroll: UniformListScrollHandle::new(),
            pending_select: None,
            dir_query: Query::new(stale),
            history: Vec::new(),
            history_pos: 0,
            copied: None,
            sort_field,
            sort_order,
            sort_menu_open: false,
            paths,
            store,
            settings_open: false,
            context: None,
            jobs: Vec::new(),
            job_seq: 0,
            jobs_open: false,
            jobs_maximized: false,
            rc_health: RcHealth::Unknown,
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
        Self::poll_jobs(window, cx);
        this
    }

    /// Poll rclone every second for the state and progress of active jobs.
    fn poll_jobs(window: &Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let snapshot = cx.update(|_, app| {
                    this.update(app, |v, _| {
                        let active: Vec<(usize, String, u64)> = v
                            .jobs
                            .iter()
                            .filter(|j| !j.done && j.jobid.is_some())
                            .map(|j| (j.id, j.group.clone(), j.jobid.unwrap()))
                            .collect();
                        (v.service.clone(), active)
                    })
                    .ok()
                });
                let (service, active) = match snapshot {
                    Ok(Some(s)) => s,
                    _ => break,
                };
                for (id, group, jobid) in active {
                    let status = service.job_status(jobid).await.ok();
                    let stats = service.stats(group).await.ok();
                    let alive = cx.update(|_, app| {
                        this.update(app, |v, vcx| {
                            if let Some(j) = v.jobs.iter_mut().find(|j| j.id == id) {
                                if let Some(s) = &stats {
                                    j.bytes = s.bytes;
                                    j.total = s.total_bytes;
                                    j.speed = s.speed;
                                }
                                if let Some(st) = &status {
                                    if st.finished {
                                        j.done = true;
                                        if !st.success {
                                            j.error = Some(if st.error.is_empty() {
                                                "failed".into()
                                            } else {
                                                st.error.clone()
                                            });
                                        }
                                    }
                                }
                            }
                            vcx.notify();
                        })
                        .is_ok()
                    });
                    if !matches!(alive, Ok(true)) {
                        return;
                    }
                }
            }
        })
        .detach();
    }

    /// Ping the rc daemon every few seconds and reflect reachability in the
    /// status-bar dot. Runs regardless of window focus so a dropped daemon is
    /// noticed promptly.
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
                            v.rc_health = health;
                            vcx.notify();
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
                if let Ok(remotes) = result {
                    this.remotes = remotes;
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
        let service = self.service.clone();
        let (field, order) = (self.sort_field, self.sort_order);
        self.dir_query.reload(cx, |this| &mut this.dir_query, move |(remote, path)| async move {
            let mut entries = service.list_dir(&remote, &path).await?;
            sort_entries(&mut entries, field, order);
            Ok::<_, ServiceError>(entries)
        });
    }

    fn choose_sort(&mut self, field: SortField, cx: &mut Context<Self>) {
        if self.sort_field == field {
            self.sort_order = self.sort_order.toggle();
        } else {
            self.sort_field = field;
        }
        self.sort_menu_open = false;
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
            self.remote_sel = ix;
            let name = remote.name.clone();
            self.navigate(name, String::new(), None, cx);
        }
    }

    fn is_pinned(&self, name: &str) -> bool {
        self.store.get().pinned.iter().any(|n| n == name)
    }

    /// Pinned remotes (in pinned order), then the rest in their existing sort.
    fn pinned_remotes(&self) -> Vec<RemoteInfo> {
        self.store
            .get()
            .pinned
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
        self.store.update(|s| match s.pinned.iter().position(|n| n == &name) {
            Some(pos) => {
                s.pinned.remove(pos);
            }
            None => s.pinned.push(name.clone()),
        });
        self.restore_remote_sel(selected);
        cx.notify();
    }

    /// Move pinned `from` to sit before pinned `before` (drop-to-reorder).
    fn reorder_pinned(&mut self, from: &str, before: &str, cx: &mut Context<Self>) {
        if from == before {
            return;
        }
        let selected = self.ordered_remotes().get(self.remote_sel).map(|r| r.name.clone());
        self.store.update(|s| {
            let Some(fp) = s.pinned.iter().position(|n| n == from) else {
                return;
            };
            let name = s.pinned.remove(fp);
            let ip = s.pinned.iter().position(|n| n == before).unwrap_or(s.pinned.len());
            s.pinned.insert(ip, name);
        });
        self.restore_remote_sel(selected);
        cx.notify();
    }

    /// Shift a pinned remote one slot up or down within the pinned group.
    fn move_pinned(&mut self, name: &str, up: bool, cx: &mut Context<Self>) {
        let selected = self.ordered_remotes().get(self.remote_sel).map(|r| r.name.clone());
        self.store.update(|s| {
            let Some(i) = s.pinned.iter().position(|n| n == name) else {
                return;
            };
            let j = if up {
                i.checked_sub(1)
            } else {
                (i + 1 < s.pinned.len()).then_some(i + 1)
            };
            if let Some(j) = j {
                s.pinned.swap(i, j);
            }
        });
        self.restore_remote_sel(selected);
        cx.notify();
    }

    fn restore_remote_sel(&mut self, name: Option<String>) {
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
        }
    }

    /// Push a new location onto history, selecting `want` (by name) on arrival.
    /// Saves the current row first so going back restores it.
    fn navigate(&mut self, remote: String, path: String, want: Option<String>, cx: &mut Context<Self>) {
        self.remember_sel();
        self.open_remote = Some(remote.clone());
        self.path = path.clone();
        self.history.truncate(self.history_pos + 1);
        self.history.push(Location { remote, path, selected: None });
        self.history_pos = self.history.len() - 1;
        self.entry_sel = 0;
        self.pending_select = want;
        self.load_entries(cx);
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
        self.pending_select = loc.selected;
        self.load_entries(cx);
    }

    /// Apply a pending select-by-name once its listing has loaded, then clamp.
    fn resolve_selection(&mut self) {
        if self.dir_query.data().is_none() {
            return;
        }
        if let Some(name) = self.pending_select.take() {
            if let Some(idx) = self.entries().iter().position(|e| e.name == name) {
                self.entry_sel = idx;
                self.scroll_to_selection();
            }
        }
        let len = self.entries().len();
        if len > 0 && self.entry_sel >= len {
            self.entry_sel = len - 1;
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
            || self.sort_menu_open
            || self.context.is_some()
            || self.remote_menu.is_some()
            || self.jobs_open
        {
            self.settings_open = false;
            self.jobs_open = false;
            self.close_menus();
            cx.notify();
        }
    }

    fn set_refresh(&mut self, secs: u64, cx: &mut Context<Self>) {
        self.store.update(|s| s.refresh_secs = secs);
        self.dir_query.set_stale_after(Duration::from_secs(secs.max(1)));
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

    fn download_entry(&mut self, entry: &Entry, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        let dest = self.store.get().download_dir();
        let id = self.job_seq;
        self.job_seq += 1;
        let group = format!("rspace/{id}");
        self.jobs.push(Job {
            id,
            group: group.clone(),
            jobid: None,
            title: format!("Download {}", entry.name),
            done: false,
            error: None,
            bytes: 0,
            total: 0,
            speed: 0.0,
        });
        cx.notify();

        let service = self.service.clone();
        let path = entry.path.clone();
        cx.spawn(async move |this, cx| {
            let result = service.download(remote, path, dest, group).await;
            this.update(cx, |this, cx| {
                if let Some(j) = this.jobs.iter_mut().find(|j| j.id == id) {
                    match result {
                        Ok(jobid) => j.jobid = Some(jobid),
                        Err(e) => {
                            j.done = true;
                            j.error = Some(e.to_string());
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn cancel_job(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(jobid) = self.jobs.iter().find(|j| j.id == id).and_then(|j| j.jobid) else {
            return;
        };
        let service = self.service.clone();
        cx.spawn(async move |this, cx| {
            let _ = service.job_stop(jobid).await;
            this.update(cx, |this, cx| {
                if let Some(j) = this.jobs.iter_mut().find(|j| j.id == id) {
                    j.done = true;
                    j.error.get_or_insert_with(|| "cancelled".into());
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn clear_finished(&mut self, cx: &mut Context<Self>) {
        self.jobs.retain(|j| !j.done);
        if self.jobs.is_empty() {
            self.jobs_open = false;
        }
        cx.notify();
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

    fn select_next(&mut self, _: &SelectNext, _window: &mut Window, cx: &mut Context<Self>) {
        let len = self.active_len();
        let sel = match self.pane {
            Pane::Sidebar => &mut self.remote_sel,
            Pane::Explorer => &mut self.entry_sel,
        };
        if *sel + 1 < len {
            *sel += 1;
            cx.notify();
        }
        self.scroll_to_selection();
    }

    fn select_prev(&mut self, _: &SelectPrev, _window: &mut Window, cx: &mut Context<Self>) {
        let sel = match self.pane {
            Pane::Sidebar => &mut self.remote_sel,
            Pane::Explorer => &mut self.entry_sel,
        };
        *sel = sel.saturating_sub(1);
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
            let parent = match self.path.rsplit_once('/') {
                Some((parent, _)) => parent.to_string(),
                None => String::new(),
            };
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

    fn copy_path(&mut self, cx: &mut Context<Self>) {
        self.copy_with_feedback(CopySource::Path, self.copy_text(), cx);
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

    fn copy_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let copied = self.copied == Some(CopySource::Path);
        h_flex()
            .id("copy-path")
            .size(px(22.0))
            .ml_1()
            .flex_shrink_0()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .hover(|s| s.bg(rgba(OVERLAY)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.copy_path(cx)))
            .child(
                svg()
                    .path(if copied { "icons/check.svg" } else { "icons/copy.svg" })
                    .size(px(13.0))
                    .flex_shrink_0()
                    .text_color(if copied { rgb(SUCCESS) } else { rgb(FG_MUTED) }),
            )
    }

    fn render_error(&self, message: String, cx: &mut Context<Self>) -> impl IntoElement {
        let copied = self.copied == Some(CopySource::Error);
        let to_copy = message.clone();
        v_flex().size_full().justify_center().items_center().p_8().child(
            v_flex()
                .max_w(px(440.0))
                .items_center()
                .gap_3()
                .child(svg().path("icons/alert.svg").size(px(28.0)).text_color(rgb(DANGER)))
                .child(div().text_color(rgb(FG)).child("Failed to load"))
                .child(
                    div()
                        .w_full()
                        .max_h(px(180.0))
                        .overflow_hidden()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(INSET))
                        .border_1()
                        .border_color(rgb(BORDER_MUTED))
                        .text_xs()
                        .text_color(rgb(FG_MUTED))
                        .child(message),
                )
                .child(
                    h_flex().w_full().justify_end().child(
                        h_flex()
                            .id("copy-error")
                            .gap_1p5()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_xs()
                            .text_color(if copied { rgb(SUCCESS) } else { rgb(FG_MUTED) })
                            .hover(|s| s.bg(rgba(OVERLAY)))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.copy_with_feedback(CopySource::Error, to_copy.clone(), cx)
                            }))
                            .child(
                                svg()
                                    .path(if copied { "icons/check.svg" } else { "icons/copy.svg" })
                                    .size(px(13.0))
                                    .text_color(if copied { rgb(SUCCESS) } else { rgb(FG_MUTED) }),
                            )
                            .child(if copied { "Copied" } else { "Copy" }),
                    ),
                ),
        )
    }

    // Battle-tested overflow: collapse the middle when the path is deep
    // (remote › … › parent › current); each segment truncates with ellipsis.
    fn render_breadcrumb(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let container = h_flex().gap_1().min_w(px(0.0));
        let Some(remote) = self.open_remote.clone() else {
            return container.child(div().text_color(rgb(FG_SUBTLE)).child("Select a remote"));
        };

        let mut segs: Vec<(String, String)> = vec![(remote.clone(), String::new())];
        if !self.path.is_empty() {
            let mut acc = String::new();
            for part in self.path.split('/') {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(part);
                segs.push((part.to_string(), acc.clone()));
            }
        }

        let n = segs.len();
        let visible: Vec<(usize, bool)> = if n <= MAX_CRUMBS {
            (0..n).map(|i| (i, false)).collect()
        } else {
            vec![(0, false), (n - 3, true), (n - 2, false), (n - 1, false)]
        };

        let mut row = container;
        for (pos, (idx, ellipsis)) in visible.into_iter().enumerate() {
            if pos > 0 {
                row = row.child(div().flex_shrink_0().text_color(rgb(FG_SUBTLE)).child("›"));
            }
            let (label, path) = segs[idx].clone();
            let label = if ellipsis { "…".to_string() } else { label };
            let is_last = idx == n - 1;
            let remote = remote.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("crumb-{pos}")))
                    .px_1()
                    .rounded_md()
                    .flex_shrink_0()
                    .max_w(px(160.0))
                    .truncate()
                    .cursor_pointer()
                    .text_color(if is_last { rgb(FG) } else { rgb(FG_MUTED) })
                    .hover(|s| s.bg(rgba(OVERLAY)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.navigate(remote.clone(), path.clone(), None, cx)
                    }))
                    .child(label),
            );
        }
        row.child(self.copy_button(cx))
    }

    // Deferred absolute overlay straddling the sidebar's right border (Zed's
    // dock pattern): the border is the flush divider, the handle takes no layout
    // space, and `deferred` lets it paint/hit-test on top of the next pane.
    fn resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        deferred(
            div()
                .id("sidebar-resize")
                .absolute()
                .top(px(0.0))
                .right(px(-RESIZE_HANDLE_W / 2.0))
                .w(px(RESIZE_HANDLE_W))
                .h_full()
                .cursor_col_resize()
                .occlude()
                .on_drag(DragSidebar, |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| DragSidebar)
                })
                .on_click(cx.listener(|this, e: &ClickEvent, _, cx| {
                    if e.click_count() >= 2 {
                        this.sidebar_width = px(SIDEBAR_W);
                        cx.notify();
                    }
                })),
        )
    }

    fn render_sort(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = format!("{} {}", self.sort_field.label(), sort_arrow(self.sort_order));
        h_flex()
            .id("sort-button")
            .gap_1()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_color(rgb(FG_MUTED))
            .hover(|s| s.bg(rgba(OVERLAY)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.sort_menu_open = !this.sort_menu_open;
                cx.notify();
            }))
            .child(label)
            .when(self.sort_menu_open, |b| {
                b.child(
                    deferred(
                        anchored()
                            .anchor(Anchor::TopRight)
                            .snap_to_window_with_margin(px(8.0))
                            .child(self.sort_menu(cx)),
                    )
                    .priority(1),
                )
            })
    }

    fn sort_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("sort-menu")
            .occlude()
            .mt(px(22.0))
            .min_w(px(160.0))
            .p_1()
            .rounded_md()
            .bg(rgb(ELEVATED))
            .border_1()
            .border_color(rgb(BORDER_MUTED))
            .shadow_lg()
            .text_color(rgb(FG))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.sort_menu_open = false;
                cx.notify();
            }))
            .child(self.sort_item(SortField::Name, cx))
            .child(self.sort_item(SortField::Size, cx))
            .child(self.sort_item(SortField::Modified, cx))
    }

    fn sort_item(&self, field: SortField, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.sort_field == field;
        let arrow = if active { sort_arrow(self.sort_order) } else { "" };
        h_flex()
            .id(field.label())
            .w_full()
            .justify_between()
            .gap_4()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_color(if active { rgb(FG) } else { rgb(FG_MUTED) })
            .hover(|s| s.bg(rgba(SELECT_MUTED)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.choose_sort(field, cx)))
            .child(field.label())
            .child(div().text_color(rgb(ACCENT)).child(arrow))
    }

    fn remote_row(
        &self,
        ix: usize,
        remote: RemoteInfo,
        pinned: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let focused = self.pane == Pane::Sidebar;
        let selected = ix == self.remote_sel;
        let menu_name = remote.name.clone();
        let mut row = list_item(ix, selected, focused)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.load_remote(ix, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    this.remote_menu = Some((menu_name.clone(), ev.position));
                    cx.notify();
                }),
            )
            // A pinned remote leads with the pin glyph, mirroring the name's color.
            .when(pinned, |r| {
                r.child(svg().path("icons/pin.svg").size(px(12.0)).flex_shrink_0().text_color(rgb(ACCENT)))
            })
            .child(
                div()
                    .flex_grow(1.0)
                    .min_w(px(0.0))
                    .truncate()
                    .text_color(rgb(FG))
                    .child(remote.name.clone()),
            )
            .child(div().text_xs().flex_shrink_0().text_color(rgb(FG_SUBTLE)).child(remote.kind.clone()));

        if pinned {
            let drag_name = remote.name.clone();
            let target = remote.name.clone();
            row = row
                .on_drag(DraggedRemote { name: drag_name }, |d, _, _, app| {
                    app.new(|_| DragLabel { text: d.name.clone().into() })
                })
                .drag_over::<DraggedRemote>(|s, _, _, _| s.bg(rgba(SELECT_MUTED)))
                .on_drop(cx.listener(move |this, d: &DraggedRemote, _, cx| {
                    this.reorder_pinned(&d.name, &target, cx)
                }));
        }
        row
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.remotes.len();
        v_flex()
            .relative()
            .w(self.sidebar_width)
            .flex_shrink_0()
            .overflow_hidden()
            .bg(rgb(INSET))
            .border_r_1()
            .border_color(rgb(BORDER_MUTED))
            .child(self.resize_handle(cx))
            .child(div().px_3().py_2().text_xs().text_color(rgb(FG_SUBTLE)).child("REMOTES"))
            .child(
                // One list; pinned remotes simply lead it (Telegram-style), so
                // they scroll with everything else when there are many.
                uniform_list(
                    "remotes",
                    count,
                    cx.processor(|this, range: Range<usize>, _window, cx| {
                        let ordered = this.ordered_remotes();
                        let pinned_count = this.pinned_remotes().len();
                        range
                            .filter_map(|ix| ordered.get(ix).map(|r| (ix, r.clone())))
                            .map(|(ix, remote)| this.remote_row(ix, remote, ix < pinned_count, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.remote_scroll)
                .flex_1(),
            )
    }

    fn render_explorer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.entries().len();
        let count_text = match self.dir_query.status() {
            _ if self.open_remote.is_none() => String::new(),
            Status::Error(_) => String::new(),
            _ => format!("{count} items"),
        };

        let body = if self.open_remote.is_none() {
            centered("Select a remote to browse", FG_SUBTLE).into_any_element()
        } else if matches!(self.dir_query.status(), Status::Loading) {
            loading_view().into_any_element()
        } else if let Status::Error(message) = self.dir_query.status() {
            self.render_error(message.clone(), cx).into_any_element()
        } else {
            uniform_list(
                "entries",
                count,
                cx.processor(|this, range: Range<usize>, _window, cx| {
                    let focused = this.pane == Pane::Explorer;
                    range
                        .filter_map(|ix| this.entries().get(ix).map(|e| (ix, e.clone())))
                        .map(|(ix, entry)| {
                            let selected = ix == this.entry_sel;
                            let is_dir = entry.is_dir;
                            let size_label = human_size(entry.size);
                            let name = entry.name.clone();
                            let ctx_entry = entry.clone();
                            list_item(ix, selected, focused)
                                .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                                    this.entry_sel = ix;
                                    this.pane = Pane::Explorer;
                                    this.context = None;
                                    if ev.click_count() >= 2 {
                                        this.descend(ix, cx);
                                    } else {
                                        cx.notify();
                                    }
                                }))
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                        this.entry_sel = ix;
                                        this.pane = Pane::Explorer;
                                        this.context = Some((ctx_entry.clone(), ev.position));
                                        cx.notify();
                                    }),
                                )
                                .child(
                                    h_flex()
                                        .id(SharedString::from(format!("name-{ix}")))
                                        .gap_2()
                                        .flex_grow(1.0)
                                        .min_w(px(0.0))
                                        .tooltip(tooltip_text(name.clone()))
                                        .child(file_icon(is_dir))
                                        .child(div().truncate().child(name)),
                                )
                                .child(if is_dir {
                                    div()
                                } else {
                                    div().text_xs().text_color(rgb(FG_MUTED)).child(size_label)
                                })
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.entry_scroll)
            .flex_1()
            .into_any_element()
        };

        v_flex()
            .flex_1()
            .bg(rgb(CANVAS))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_between()
                    .pl_1()
                    .pr_3()
                    .py_1()
                    .border_b_1()
                    .border_color(rgb(BORDER_MUTED))
                    .child(
                        h_flex()
                            .gap_1()
                            .min_w(px(0.0))
                            .child(nav_button("nav-back", "←", self.can_back()).when(
                                self.can_back(),
                                |b| b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go_back(cx))),
                            ))
                            .child(nav_button("nav-forward", "→", self.can_forward()).when(
                                self.can_forward(),
                                |b| b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go_forward(cx))),
                            ))
                            .child(self.render_breadcrumb(cx)),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(rgb(FG_MUTED))
                            .when(self.dir_query.is_fetching(), |el| {
                                el.child(spinner("fetch-spinner", px(12.0), FG_MUTED))
                            })
                            .child(count_text)
                            .when(self.open_remote.is_some(), |el| {
                                el.child(self.render_sort(cx))
                            }),
                    ),
            )
            .child(body)
    }

    fn render_title_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lead = if window.is_fullscreen() { 12.0 } else { TITLE_BAR_LEAD };
        h_flex()
            .h(px(TITLE_BAR_H))
            .flex_shrink_0()
            .w_full()
            .pl(px(lead))
            .pr_2()
            .justify_end()
            .bg(rgb(INSET))
            .border_b_1()
            .border_color(rgb(BORDER_MUTED))
            .child(
                h_flex()
                    .id("settings-button")
                    .size(px(24.0))
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(OVERLAY)))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.settings_open = true;
                        cx.notify();
                    }))
                    .child(svg().path("icons/settings.svg").size(px(16.0)).text_color(rgb(FG_MUTED))),
            )
    }

    fn menu_item(
        &self,
        label: &'static str,
        icon: &'static str,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        h_flex()
            .id(label)
            .w_full()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .hover(|s| s.bg(rgba(OVERLAY)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                action(this, cx);
                this.close_menus();
                cx.notify();
            }))
            .child(svg().path(icon).size(px(15.0)).flex_shrink_0().text_color(rgb(FG_MUTED)))
            .child(label)
    }

    /// Clear every transient popover. Menu actions call this so the menu closes
    /// regardless of which one it belongs to.
    fn close_menus(&mut self) {
        self.context = None;
        self.remote_menu = None;
        self.sort_menu_open = false;
    }

    /// Float `items` as a popover anchored at `pos`. The surface occludes the
    /// mouse — the root fix for hover/click bleeding through to content behind —
    /// and dismisses on an outside mouse-down. Every right-click menu goes
    /// through here so behaviour stays uniform.
    fn popover(
        &self,
        id: &'static str,
        pos: Point<Pixels>,
        items: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let menu = v_flex()
            .id(id)
            .occlude()
            .min_w(px(180.0))
            .p_1()
            .rounded_md()
            .bg(rgb(ELEVATED))
            .border_1()
            .border_color(rgb(BORDER_MUTED))
            .shadow_lg()
            .text_color(rgb(FG))
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.close_menus();
                cx.notify();
            }))
            .children(items);
        deferred(anchored().position(pos).snap_to_window_with_margin(px(8.0)).child(menu)).priority(2)
    }

    fn render_context_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (entry, pos) = self.context.clone().unwrap();
        let remote = self.open_remote.clone().unwrap_or_default();
        let mut items: Vec<AnyElement> = Vec::new();

        if entry.is_dir {
            let (e, r) = (entry.clone(), remote.clone());
            items.push(
                self.menu_item("Open", "icons/folder_open.svg", cx, move |this, cx| {
                    this.navigate(r.clone(), e.path.clone(), None, cx)
                })
                .into_any_element(),
            );
        }
        let e_dl = entry.clone();
        let (e_cp, r_cp) = (entry.clone(), remote.clone());
        let e_nm = entry.clone();
        items.push(
            self.menu_item("Download", "icons/download.svg", cx, move |this, cx| {
                this.download_entry(&e_dl, cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item("Copy path", "icons/copy.svg", cx, move |this, cx| {
                this.copy_to_clipboard(format!("{}:{}", r_cp, e_cp.path), cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item("Copy name", "icons/copy.svg", cx, move |this, cx| {
                this.copy_to_clipboard(e_nm.name.clone(), cx)
            })
            .into_any_element(),
        );

        self.popover("context-menu", pos, items, cx)
    }

    fn render_remote_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (name, pos) = self.remote_menu.clone().unwrap();
        let pinned = self.is_pinned(&name);
        let mut items: Vec<AnyElement> = Vec::new();

        let open_name = name.clone();
        items.push(
            self.menu_item("Open", "icons/folder_open.svg", cx, move |this, cx| {
                if let Some(ix) = this.ordered_remotes().iter().position(|r| r.name == open_name) {
                    this.load_remote(ix, cx);
                }
            })
            .into_any_element(),
        );

        let pin_name = name.clone();
        let (pin_label, pin_icon) = if pinned { ("Unpin", "icons/pin.svg") } else { ("Pin", "icons/pin.svg") };
        items.push(
            self.menu_item(pin_label, pin_icon, cx, move |this, cx| {
                this.toggle_pin(pin_name.clone(), cx)
            })
            .into_any_element(),
        );

        if pinned {
            let up_name = name.clone();
            let down_name = name.clone();
            items.push(
                self.menu_item("Move up", "icons/chevron_up.svg", cx, move |this, cx| {
                    this.move_pinned(&up_name, true, cx)
                })
                .into_any_element(),
            );
            items.push(
                self.menu_item("Move down", "icons/chevron_down.svg", cx, move |this, cx| {
                    this.move_pinned(&down_name, false, cx)
                })
                .into_any_element(),
            );
        }

        self.popover("remote-menu", pos, items, cx)
    }

    // Bottom dock panel (Zed-style): the browser stays visible above it.
    fn render_transfers(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_done = self.jobs.iter().any(|j| j.done);
        let count = self.jobs.len();
        let body = if count == 0 {
            centered("No transfers", FG_SUBTLE).into_any_element()
        } else {
            uniform_list(
                "transfers",
                count,
                cx.processor(|this, range: Range<usize>, _window, cx| {
                    let n = this.jobs.len();
                    range
                        // Newest first.
                        .filter_map(|i| {
                            n.checked_sub(1 + i).and_then(|idx| this.jobs.get(idx).cloned()).map(|j| (i, j))
                        })
                        .map(|(i, job)| {
                            div()
                                .px_3()
                                .when(i > 0, |d| d.border_t_1().border_color(rgb(BORDER_MUTED)))
                                .child(this.job_row(&job, cx))
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .flex_1()
            .into_any_element()
        };

        let maximized = self.jobs_maximized;
        let outer = if maximized {
            v_flex().flex_1().min_h(px(0.0))
        } else {
            v_flex().h(px(260.0)).flex_shrink_0()
        };
        outer
            .bg(rgb(INSET))
            // Maximized sits flush under the title bar, which already draws the
            // boundary; only the docked panel needs its own top border.
            .when(!maximized, |el| el.border_t_1().border_color(rgb(BORDER_MUTED)))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(rgb(BORDER_MUTED))
                    .child(div().text_color(rgb(FG)).child("Transfers"))
                    .child(
                        h_flex()
                            .gap_1()
                            .when(has_done, |el| {
                                el.child(
                                    h_flex()
                                        .id("clear-finished")
                                        .px_2()
                                        .py_1()
                                        .mr_1()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(rgb(FG_MUTED))
                                        .hover(|s| s.bg(rgba(OVERLAY)))
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.clear_finished(cx)
                                        }))
                                        .child("Clear finished"),
                                )
                            })
                            .child(
                                icon_button(
                                    "transfers-maximize",
                                    if maximized { "icons/minimize.svg" } else { "icons/maximize.svg" },
                                )
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.jobs_maximized = !this.jobs_maximized;
                                    cx.notify();
                                })),
                            )
                            .child(
                                h_flex()
                                    .id("transfers-close")
                                    .size(px(22.0))
                                    .justify_center()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .text_color(rgb(FG_MUTED))
                                    .hover(|s| s.bg(rgba(OVERLAY)))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.jobs_open = false;
                                        cx.notify();
                                    }))
                                    .child("✕"),
                            ),
                    ),
            )
            .child(body)
    }

    fn job_row(&self, job: &Job, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let pct = if job.total > 0 {
            (job.bytes as f64 / job.total as f64).clamp(0.0, 1.0)
        } else if job.done && job.error.is_none() {
            1.0
        } else {
            0.0
        };
        let bar = if job.error.is_some() {
            DANGER
        } else if job.done {
            SUCCESS
        } else {
            ACCENT
        };
        let detail = if let Some(e) = &job.error {
            e.clone()
        } else if job.done {
            "Done".to_string()
        } else if job.total > 0 {
            format!(
                "{} / {} · {}/s",
                human_size(job.bytes as i64),
                human_size(job.total as i64),
                human_size(job.speed as i64)
            )
        } else {
            "Starting…".to_string()
        };
        let id = job.id;

        v_flex()
            .gap_1p5()
            .py_2p5()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex_grow(1.0)
                            .min_w(px(0.0))
                            .truncate()
                            .text_color(rgb(FG))
                            .child(job.title.clone()),
                    )
                    .when(!job.done, |el| {
                        el.child(
                            h_flex()
                                .id(SharedString::from(format!("cancel-{id}")))
                                .px_2()
                                .rounded_md()
                                .cursor_pointer()
                                .text_xs()
                                .text_color(rgb(FG_MUTED))
                                .hover(|s| s.bg(rgba(OVERLAY)))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.cancel_job(id, cx)
                                }))
                                .child("Cancel"),
                        )
                    }),
            )
            .child(
                div()
                    .w_full()
                    .h(px(4.0))
                    .rounded_full()
                    .bg(rgba(OVERLAY))
                    .child(div().h_full().rounded_full().bg(rgb(bar)).w(relative(pct as f32))),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(if job.error.is_some() { rgb(DANGER) } else { rgb(FG_MUTED) })
                    .child(detail),
            )
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        deferred(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .justify_center()
                .items_center()
                .bg(rgba(0x0000_0099))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.settings_open = false;
                        cx.notify();
                    }),
                )
                .child(
                    v_flex()
                        .id("settings-card")
                        .w(px(460.0))
                        .gap_5()
                        .p_5()
                        .rounded_lg()
                        .bg(rgb(ELEVATED))
                        .border_1()
                        .border_color(rgb(BORDER_MUTED))
                        .shadow_lg()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|_, _: &MouseDownEvent, _, cx| cx.stop_propagation()),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .items_center()
                                .child(div().text_lg().text_color(rgb(FG)).child("Settings"))
                                .child(
                                    h_flex()
                                        .id("settings-close")
                                        .size(px(24.0))
                                        .justify_center()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .text_color(rgb(FG_MUTED))
                                        .hover(|s| s.bg(rgba(OVERLAY)))
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.settings_open = false;
                                            cx.notify();
                                        }))
                                        .child("✕"),
                                ),
                        )
                        .child(self.refresh_setting(cx))
                        .child(self.download_setting(cx))
                        .child(self.settings_info()),
                ),
        )
        .priority(2)
    }

    fn download_setting(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.store.get().download_dir().display().to_string();
        setting_block(
            "Download location",
            "Where files are saved. Defaults to your Downloads folder.",
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_grow(1.0)
                        .min_w(px(0.0))
                        .truncate()
                        .text_xs()
                        .text_color(rgb(FG_MUTED))
                        .child(current),
                )
                .child(
                    h_flex()
                        .id("choose-dir")
                        .flex_shrink_0()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .bg(rgba(OVERLAY))
                        .text_color(rgb(FG))
                        .hover(|s| s.bg(rgba(SELECT_MUTED)))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.choose_download_dir(cx)
                        }))
                        .child("Choose…"),
                ),
        )
    }

    fn refresh_setting(&self, cx: &mut Context<Self>) -> impl IntoElement {
        setting_block(
            "Refresh interval",
            "How often open folders revalidate in the background.",
            h_flex()
                .gap_1()
                .child(self.refresh_preset(5, cx))
                .child(self.refresh_preset(15, cx))
                .child(self.refresh_preset(30, cx))
                .child(self.refresh_preset(60, cx)),
        )
    }

    fn refresh_preset(&self, secs: u64, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.store.get().refresh_secs == secs;
        let base = h_flex()
            .id(SharedString::from(format!("preset-{secs}")))
            .px_3()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_color(if active { rgb(FG) } else { rgb(FG_MUTED) })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_refresh(secs, cx)))
            .child(format!("{secs}s"));
        if active {
            base.bg(rgba(SELECT))
        } else {
            base.hover(|s| s.bg(rgba(OVERLAY)))
        }
    }

    fn settings_info(&self) -> impl IntoElement {
        v_flex()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(rgb(BORDER_MUTED))
            .child(info_row("rclone", &self.version))
            .child(info_row("Data", &self.paths.root().display().to_string()))
            .child(info_row("Config", &self.paths.config_dir().display().to_string()))
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let info = if self.open_remote.is_some() {
            format!("{} items", self.entries().len())
        } else {
            format!("{} remotes", self.remotes.len())
        };
        h_flex()
            .w_full()
            .flex_shrink_0()
            .justify_between()
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(rgb(BORDER_MUTED))
            .bg(rgb(INSET))
            .text_xs()
            .text_color(rgb(FG_MUTED))
            .child(
                h_flex().gap_2().child(self.health_dot()).children(self.active_remote().map(|r| {
                    h_flex()
                        .gap_2()
                        .child(div().text_color(rgb(FG)).child(r.name.clone()))
                        .child(div().text_color(rgb(FG_SUBTLE)).child(r.kind.clone()))
                })),
            )
            .child(
                h_flex()
                    .gap_3()
                    .when(!self.jobs.is_empty(), |el| el.child(self.jobs_indicator(cx)))
                    .child(info)
                    .child(self.version.clone()),
            )
    }

    fn health_dot(&self) -> impl IntoElement {
        let (color, tip) = match &self.rc_health {
            RcHealth::Unknown => (FG_SUBTLE, "Checking rclone daemon…".to_string()),
            RcHealth::Up if self.version.is_empty() => (SUCCESS, "rclone rc daemon connected".to_string()),
            RcHealth::Up => (SUCCESS, format!("rclone {} · rc daemon connected", self.version)),
            RcHealth::Down(e) => (DANGER, format!("rclone rc daemon unreachable: {e}")),
        };
        h_flex()
            .id("rc-health")
            .child(div().text_color(rgb(color)).child("●"))
            .tooltip(tooltip_text(tip))
    }

    fn jobs_indicator(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.jobs.iter().filter(|j| !j.done).count();
        let label = if active > 0 {
            format!("↓ {active}")
        } else {
            format!("✓ {}", self.jobs.len())
        };
        h_flex()
            .id("jobs-indicator")
            .gap_1()
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .text_color(if active > 0 { rgb(FG) } else { rgb(FG_MUTED) })
            .hover(|s| s.bg(rgba(OVERLAY)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.jobs_open = !this.jobs_open;
                cx.notify();
            }))
            .child(label)
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.resolve_selection();
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
            .on_drag_move(cx.listener(|this, e: &DragMoveEvent<DragSidebar>, _, cx| {
                let x = f32::from(e.event.position.x).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                if px(x) != this.sidebar_width {
                    this.sidebar_width = px(x);
                    cx.notify();
                }
            }))
            .size_full()
            .bg(rgb(CANVAS))
            .text_color(rgb(FG))
            .text_sm()
            .child(self.render_title_bar(window, cx))
            .child({
                // A panel only covers the browser while it is open AND zoomed
                // (Zed's dock-zoom model); closing it always reveals the browser,
                // so zoom state can never leave the content region blank.
                let zoomed = self.jobs_open && self.jobs_maximized;
                v_flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .when(!zoomed, |el| {
                        el.child(
                            // Plain flex_row (not h_flex): panes must stretch to full
                            // height, not center on the cross-axis.
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
            .when(self.settings_open, |el| el.child(self.render_settings(cx)))
    }
}

/// One labeled setting: title, description, and its control. Adding a setting
/// to the page is one of these.
fn setting_block(title: &str, desc: &str, control: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_2()
        .child(div().text_sm().text_color(rgb(FG)).child(title.to_string()))
        .child(div().text_xs().text_color(rgb(FG_MUTED)).child(desc.to_string()))
        .child(control)
}

fn info_row(label: &str, value: &str) -> impl IntoElement {
    h_flex()
        .w_full()
        .justify_between()
        .gap_4()
        .text_xs()
        .child(div().flex_shrink_0().text_color(rgb(FG_MUTED)).child(label.to_string()))
        .child(
            div()
                .min_w(px(0.0))
                .truncate()
                .text_color(rgb(FG_SUBTLE))
                .child(value.to_string()),
        )
}

fn centered(text: &'static str, color: u32) -> Div {
    v_flex().size_full().justify_center().items_center().text_color(rgb(color)).child(text)
}

fn spinner(id: &'static str, size: Pixels, color: u32) -> impl IntoElement {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    div().text_size(size).text_color(rgb(color)).with_animation(
        id,
        Animation::new(Duration::from_millis(800)).repeat(),
        |el, delta| {
            let i = ((delta * FRAMES.len() as f32) as usize).min(FRAMES.len() - 1);
            el.child(FRAMES[i])
        },
    )
}

fn loading_view() -> impl IntoElement {
    v_flex()
        .size_full()
        .justify_center()
        .items_center()
        .gap_3()
        .child(spinner("panel-spinner", px(28.0), ACCENT))
        .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child("Loading…"))
}

/// Directories first, then by `field`/`order` within each group.
fn sort_entries(entries: &mut [Entry], field: SortField, order: SortOrder) {
    entries.sort_by(|a, b| {
        let within = match field {
            SortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortField::Size => a.size.cmp(&b.size),
            SortField::Modified => a.mod_time.cmp(&b.mod_time),
        };
        let within = match order {
            SortOrder::Asc => within,
            SortOrder::Desc => within.reverse(),
        };
        b.is_dir.cmp(&a.is_dir).then(within)
    });
}

fn human_size(bytes: i64) -> String {
    if bytes < 0 {
        return "—".to_string();
    }
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}
