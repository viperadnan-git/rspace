//! gpui desktop shell: a two-pane remote browser.

mod jobs;
mod menus;
mod panels;
mod query;
mod theme;
mod views;
mod widgets;

use std::collections::HashSet;
use std::ops::Range;
use std::time::Duration;

use gpui::{
    actions, anchored, deferred, div, point, prelude::*, px, relative, rgb, rgba, size, svg,
    uniform_list, Anchor, AnyElement, App, AssetSource, Bounds, ClickEvent, ClipboardItem, Context,
    Div, DragMoveEvent, FocusHandle, KeyBinding, Menu, MenuItem, MouseButton, MouseDownEvent,
    PathPromptOptions, Pixels, Point, ScrollStrategy, SharedString, Stateful, TitlebarOptions,
    UniformListScrollHandle, Window, WindowBounds, WindowOptions,
};
use gpui_platform::application;
use rspace_core::{Paths, SettingsStore, SortField, SortOrder};
use rspace_rclone_rc::{Entry, RemoteInfo, Service, ServiceError, TransferMode};

use query::{Query, Status};
use theme::*;
use widgets::*;

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
        PromptSubmit,
        PromptCancel
    ]
);

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<std::borrow::Cow<'static, [u8]>>> {
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
            "folder_open", "pin", "chevron_up", "chevron_down", "scissors", "clipboard", "refresh",
            "activity", "trash", "x", "edit"
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
        KeyBinding::new("cmd-a", SelectAll, Some("Workspace && !modal")),
        KeyBinding::new("enter", Open, Some("Workspace && !modal")),
        KeyBinding::new("tab", TogglePane, Some("Workspace && !modal")),
        KeyBinding::new("backspace", GoUp, Some("Workspace && !modal")),
        KeyBinding::new("cmd-[", GoBack, Some("Workspace && !modal")),
        KeyBinding::new("cmd-]", GoForward, Some("Workspace && !modal")),
        KeyBinding::new("cmd-r", Reload, Some("Workspace && !modal")),
        KeyBinding::new("left", FocusSidebar, Some("Workspace && !modal")),
        KeyBinding::new("right", FocusExplorer, Some("Workspace && !modal")),
        KeyBinding::new("cmd-c", CopyEntry, Some("Workspace && !modal")),
        KeyBinding::new("cmd-x", CutEntry, Some("Workspace && !modal")),
        KeyBinding::new("cmd-v", PasteEntry, Some("Workspace && !modal")),
        KeyBinding::new("cmd-backspace", DeleteEntry, Some("Workspace && !modal")),
        KeyBinding::new("cmd-shift-n", NewFolder, Some("Workspace && !modal")),
        KeyBinding::new("cmd-u", NewFile, Some("Workspace && !modal")),
        KeyBinding::new("f2", Rename, Some("Workspace && !modal")),
        KeyBinding::new("escape", CloseSettings, Some("Workspace")),
        // Confirm dialog: Enter accepts (Escape dismisses via the line above).
        KeyBinding::new("enter", ConfirmAccept, Some("Confirm")),
        // Text-input dialog: Enter submits, Escape cancels.
        KeyBinding::new("enter", PromptSubmit, Some("Prompt")),
        KeyBinding::new("escape", PromptCancel, Some("Prompt")),
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
    JobCommand(usize),
}

/// Reachability of the rclone rc daemon, surfaced by the status-bar dot.
#[derive(Clone)]
enum RcHealth {
    Unknown,
    Up,
    Down(String),
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
    /// Refresh the open listing when this job succeeds (paste changed a remote).
    reload_on_done: bool,
    /// Elapsed from rclone: live `core/stats.elapsedTime`, then `job/status.duration`.
    elapsed_ms: u64,
    /// Equivalent rclone CLI command, for the row's copy button.
    command: String,
}

/// Source for a cross-remote copy/cut, resolved against the destination at paste.
#[derive(Clone)]
struct Clipboard {
    remote: String,
    entries: Vec<Entry>,
    mode: TransferMode,
}

/// A pending confirmation dialog: its copy plus the action run once confirmed.
struct Confirm {
    title: SharedString,
    message: SharedString,
    confirm_label: SharedString,
    danger: bool,
    action: Box<dyn FnOnce(&mut Workspace, &mut Context<Workspace>)>,
}

/// An inline text edit in the explorer list (new folder / rename). `target` is
/// the path of the entry being renamed, or `None` for a new item at the top.
struct Prompt {
    value: String,
    placeholder: SharedString,
    icon_dir: bool,
    target: Option<String>,
    action: Box<dyn FnOnce(&mut Workspace, String, &mut Context<Workspace>)>,
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
    dialog_focus: FocusHandle,
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
    sort_menu_open: bool,
    paths: Paths,
    store: SettingsStore,
    settings_open: bool,
    /// Right-click context menu: the targeted entry and the cursor position.
    context: Option<(Entry, Point<Pixels>)>,
    /// Right-click on empty list space: the cursor position.
    bg_menu: Option<Point<Pixels>>,
    /// Pending confirmation dialog (destructive or irreversible actions).
    confirm: Option<Confirm>,
    /// Pending text-input dialog (new folder, rename, …).
    prompt: Option<Prompt>,
    jobs: Vec<Job>,
    job_seq: usize,
    jobs_open: bool,
    jobs_maximized: bool,
    rc_health: RcHealth,
    clipboard: Option<Clipboard>,
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
            dialog_focus: cx.focus_handle(),
            pane: Pane::Sidebar,
            remotes: Vec::new(),
            remote_sel: 0,
            remote_scroll: UniformListScrollHandle::new(),
            remote_menu: None,
            sidebar_width: px(SIDEBAR_W),
            open_remote: None,
            path: String::new(),
            entry_sel: 0,
            selected: HashSet::new(),
            sel_anchor: 0,
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
            bg_menu: None,
            confirm: None,
            prompt: None,
            jobs: Vec::new(),
            job_seq: 0,
            jobs_open: false,
            jobs_maximized: false,
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
                            let mut reload = false;
                            if let Some(j) = v.jobs.iter_mut().find(|j| j.id == id) {
                                if let Some(s) = &stats {
                                    j.bytes = s.bytes;
                                    j.total = s.total_bytes;
                                    j.speed = s.speed;
                                    j.elapsed_ms = (s.elapsed_time * 1000.0) as u64;
                                }
                                if let Some(st) = &status {
                                    if st.finished && !j.done {
                                        j.done = true;
                                        if st.duration > 0.0 {
                                            j.elapsed_ms = (st.duration * 1000.0) as u64;
                                        }
                                        if st.success {
                                            reload = j.reload_on_done;
                                            tracing::debug!(job = %j.title, elapsed_ms = j.elapsed_ms, "job done");
                                        } else {
                                            let msg = if st.error.is_empty() {
                                                "failed".to_string()
                                            } else {
                                                st.error.clone()
                                            };
                                            tracing::warn!(job = %j.title, elapsed_ms = j.elapsed_ms, error = %msg, "job failed");
                                            j.error = Some(msg);
                                        }
                                    }
                                }
                            }
                            if reload {
                                v.force_reload_entries(vcx);
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

    fn ask_confirm(
        &mut self,
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        confirm_label: impl Into<SharedString>,
        danger: bool,
        action: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.confirm = Some(Confirm {
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
            danger,
            action: Box::new(action),
        });
        cx.notify();
    }

    fn confirm_accept(&mut self, _: &ConfirmAccept, _window: &mut Window, cx: &mut Context<Self>) {
        self.run_confirm(cx);
    }

    fn run_confirm(&mut self, cx: &mut Context<Self>) {
        if let Some(c) = self.confirm.take() {
            (c.action)(self, cx);
        }
        cx.notify();
    }

    fn dismiss_confirm(&mut self, cx: &mut Context<Self>) {
        self.confirm = None;
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
        self.prompt = Some(Prompt {
            value: value.into(),
            placeholder: placeholder.into(),
            icon_dir,
            target,
            action: Box::new(action),
        });
        cx.notify();
    }

    fn prompt_submit(&mut self, _: &PromptSubmit, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = self.prompt.take() {
            let value = p.value.trim().to_string();
            if value.is_empty() {
                // Nothing entered: reopen unchanged rather than silently doing nothing.
                self.prompt = Some(p);
                return;
            }
            (p.action)(self, value, cx);
        }
        cx.notify();
    }

    fn prompt_cancel(&mut self, _: &PromptCancel, _window: &mut Window, cx: &mut Context<Self>) {
        self.prompt = None;
        cx.notify();
    }

    /// Feed a key into the open prompt's text field (printable chars + backspace).
    fn prompt_key(&mut self, ev: &gpui::KeyDownEvent, cx: &mut Context<Self>) {
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        match ev.keystroke.key.as_str() {
            "backspace" => {
                p.value.pop();
                cx.notify();
            }
            _ => {
                let m = ev.keystroke.modifiers;
                if m.platform || m.control || m.function {
                    return;
                }
                if let Some(ch) = &ev.keystroke.key_char {
                    p.value.push_str(ch);
                    cx.notify();
                }
            }
        }
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
        self.sel_anchor = 0;
        self.selected.clear();
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
        self.sel_anchor = 0;
        self.selected.clear();
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
        // Prune stale paths only for a real multi-selection (single-select is kept
        // valid by the cursor logic, so the common path allocates nothing).
        if self.selected.len() > 1 {
            let valid: HashSet<String> = self.entries().iter().map(|e| e.path.clone()).collect();
            self.selected.retain(|p| valid.contains(p));
        }
        if self.selected.is_empty() {
            if let Some(p) = self.entry_path_at(self.entry_sel) {
                self.selected.insert(p);
            }
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
            || self.bg_menu.is_some()
            || self.confirm.is_some()
            || self.prompt.is_some()
            || self.jobs_open
        {
            self.settings_open = false;
            self.jobs_open = false;
            self.confirm = None;
            self.prompt = None;
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
        let entries = self.entries();
        if self.selected.is_empty() {
            return entries.get(self.entry_sel).cloned().into_iter().collect();
        }
        entries.iter().filter(|e| self.selected.contains(&e.path)).cloned().collect()
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
                let next = (self.entry_sel + 1).min(len - 1);
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

    fn select_prev(&mut self, _: &SelectPrev, window: &mut Window, cx: &mut Context<Self>) {
        match self.pane {
            Pane::Sidebar => self.remote_sel = self.remote_sel.saturating_sub(1),
            Pane::Explorer => {
                if self.entries().is_empty() {
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
        // Keep focus on the open dialog, else on the workspace — so each owns the
        // keyboard while shown, and focus returns here when it closes.
        let want = if self.confirm.is_some() || self.prompt.is_some() {
            &self.dialog_focus
        } else {
            &self.focus
        };
        if !want.is_focused(window) {
            want.focus(window, cx);
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
                                .child(self.render_explorer(cx)),
                        )
                    })
                    .when(self.jobs_open, |el| el.child(self.render_transfers(cx)))
            })
            .child(self.render_status_bar(cx))
            .when(self.context.is_some(), |el| el.child(self.render_context_menu(cx)))
            .when(self.remote_menu.is_some(), |el| el.child(self.render_remote_menu(cx)))
            .when(self.bg_menu.is_some(), |el| el.child(self.render_bg_menu(cx)))
            .when(self.confirm.is_some(), |el| el.child(self.render_confirm(cx)))
            .when(self.settings_open, |el| el.child(self.render_settings(cx)))
    }
}

