//! File-preview pane: classify by extension, fetch over `--rc-serve` (capped,
//! cached), and render an image / text view, or a rich info card otherwise.
//!
//! A focusless child view: it observes the explorer's cursor and keeps its
//! subject in sync. The workspace owns only the pane's visibility/width.
//!
//! Adding a format: give it an arm in [`classify`] and a render branch in
//! [`PreviewPane::render`]. Adding a whole new kind: add a [`PreviewKind`]
//! variant (drives the fetch) and a [`PreviewState`] variant (drives rendering).

use std::sync::Arc;

use gpui::{Image, ImageFormat, WeakEntity};

use super::*;

/// Range cap per fetch: text is shown truncated, images skipped past the cap.
const TEXT_MAX: u64 = 256 * 1024;
const IMAGE_MAX: u64 = 16 * 1024 * 1024;
/// Decoded previews kept for instant re-selection.
const CACHE_CAP: usize = 24;

const TEXT_EXTS: &[&str] = &[
    "txt", "text", "md", "markdown", "rst", "log", "csv", "tsv", "json", "json5", "jsonc", "yaml",
    "yml", "toml", "ini", "conf", "cfg", "env", "properties", "xml", "svg", "html", "htm", "css",
    "scss", "less", "rs", "py", "pyi", "js", "jsx", "mjs", "cjs", "ts", "tsx", "go", "c", "h",
    "cc", "cpp", "cxx", "hpp", "hh", "java", "kt", "kts", "swift", "rb", "php", "pl", "lua", "sh",
    "bash", "zsh", "fish", "ps1", "bat", "sql", "graphql", "proto", "dockerfile", "makefile",
    "cmake", "gradle", "vim", "el", "clj", "ex", "exs", "erl", "hs", "ml", "scala", "dart", "r",
    "jl", "nim", "zig", "tf", "diff", "patch", "gitignore", "editorconfig",
];

/// What a file can be rendered as, decided by extension (the extension point).
#[derive(Clone, Copy)]
pub(crate) enum PreviewKind {
    Image(ImageFormat),
    Text,
}

fn classify(name: &str) -> Option<PreviewKind> {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return Some(PreviewKind::Text); // extensionless: README, LICENSE, Makefile…
    };
    Some(match ext.to_ascii_lowercase().as_str() {
        "png" => PreviewKind::Image(ImageFormat::Png),
        "jpg" | "jpeg" => PreviewKind::Image(ImageFormat::Jpeg),
        "gif" => PreviewKind::Image(ImageFormat::Gif),
        "webp" => PreviewKind::Image(ImageFormat::Webp),
        "bmp" => PreviewKind::Image(ImageFormat::Bmp),
        "tif" | "tiff" => PreviewKind::Image(ImageFormat::Tiff),
        ext if ext == "svg" => PreviewKind::Image(ImageFormat::Svg),
        ext if TEXT_EXTS.contains(&ext) => PreviewKind::Text,
        _ => return None,
    })
}

/// The rendered (or in-flight) state of the current preview.
#[derive(Clone)]
pub(crate) enum PreviewState {
    Loading,
    Image(Arc<Image>),
    Text(SharedString),
    /// Directory, or a type we can't render: show the info card only.
    Info,
    TooLarge,
    Error(SharedString),
}

/// On-demand directory size (rclone walks the tree, so it's opt-in per dir).
#[derive(Clone)]
enum DirSize {
    Idle,
    Loading,
    Done(String),
}

/// The preview subject: a selected entry, or the current directory/remote when
/// nothing is selected. Identity is `(remote, entry.path)` — qualified by remote
/// so an async result can't bind to a same-named path on a different remote.
struct Preview {
    remote: String,
    entry: Entry,
    state: PreviewState,
    dir_size: DirSize,
}

impl Preview {
    fn is(&self, remote: &str, path: &str) -> bool {
        self.remote == remote && self.entry.path == path
    }
}

/// The preview pane: tracks the explorer cursor, fetches/caches content, renders.
pub(crate) struct PreviewPane {
    workspace: WeakEntity<Workspace>,
    explorer: Entity<Explorer>,
    service: Service,
    current: Option<Preview>,
    /// Recently loaded previews, keyed by `remote:path` (LRU, bounded).
    cache: Vec<(String, PreviewState)>,
    /// Pane width (resizable; persisted by the workspace).
    width: Pixels,
}

impl PreviewPane {
    pub(crate) fn new(
        workspace: WeakEntity<Workspace>,
        explorer: Entity<Explorer>,
        service: Service,
        width: Pixels,
        cx: &mut Context<Self>,
    ) -> Self {
        // Track the cursor: every selection/navigation change notifies the
        // explorer, so observing it keeps the subject in sync.
        cx.observe(&explorer, |this, _, cx| this.refresh(cx)).detach();
        Self { workspace, explorer, service, current: None, cache: Vec::new(), width }
    }

    pub(crate) fn width(&self) -> Pixels {
        self.width
    }

    pub(crate) fn set_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        if self.width != width {
            self.width = width;
            cx.notify();
        }
    }

    pub(crate) fn reset_width(&mut self, cx: &mut Context<Self>) {
        self.set_width(px(PREVIEW_W), cx);
    }

    /// The open `(remote, path)`, read from the explorer (the source of what's
    /// shown) — never the workspace, which may be mid-update when `refresh` runs.
    fn location(&self, cx: &App) -> Option<(String, String)> {
        self.explorer.read(cx).location()
    }

    /// Keep the preview subject in sync: the selected entry, or the current
    /// directory/remote when nothing is selected (the cursor is not a selection).
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some((remote, path)) = self.location(cx) else {
            self.current = None;
            return;
        };
        let cursor = self.explorer.read(cx).cursor_entry();
        let subject_path = cursor.as_ref().map_or(path.clone(), |e| e.path.clone());
        if self.current.as_ref().is_some_and(|p| p.is(&remote, &subject_path)) {
            return;
        }
        let entry = cursor.unwrap_or_else(|| location_entry(&remote, &path));
        let key = format!("{remote}:{}", entry.path);
        if let Some(state) = self.cache_get(&key) {
            self.current = Some(Preview { remote, entry, state, dir_size: DirSize::Idle });
            cx.notify();
            return;
        }
        let state = match classify(&entry.name) {
            _ if entry.is_dir => PreviewState::Info,
            None => PreviewState::Info,
            Some(PreviewKind::Image(_)) if entry.size as u64 > IMAGE_MAX => PreviewState::TooLarge,
            Some(kind) => {
                self.spawn_fetch(remote.clone(), entry.path.clone(), kind, cx);
                PreviewState::Loading
            }
        };
        self.current = Some(Preview { remote, entry, state, dir_size: DirSize::Idle });
        cx.notify();
    }

    /// Walk the selected directory to total its size (rclone `operations/size`),
    /// shown inline in the info card. Opt-in since it can be expensive.
    fn calculate_dir_size(&mut self, cx: &mut Context<Self>) {
        let Some(preview) = self.current.as_mut() else {
            return;
        };
        let (remote, path) = (preview.remote.clone(), preview.entry.path.clone());
        preview.dir_size = DirSize::Loading;
        cx.notify();
        let args = vec![ArgValue::Path { remote: remote.clone(), path: path.clone(), is_dir: true }];
        let Some((method, params)) = InfoOp::Size.build(&args) else {
            return;
        };
        let service = self.service.clone();
        cx.spawn(async move |this, cx| {
            let res = service.query(method, params).await;
            this.update(cx, |this, cx| {
                if let Some(preview) = this.current.as_mut().filter(|p| p.is(&remote, &path)) {
                    preview.dir_size = match res.ok().and_then(|v| InfoOp::Size.parse(&v)) {
                        Some(InfoResult::Size { count, bytes }) => {
                            DirSize::Done(format!("{count} items \u{b7} {}", human_size(bytes)))
                        }
                        _ => DirSize::Done("\u{2014}".into()),
                    };
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn spawn_fetch(&self, remote: String, path: String, kind: PreviewKind, cx: &mut Context<Self>) {
        let cap = match kind {
            PreviewKind::Image(_) => IMAGE_MAX,
            PreviewKind::Text => TEXT_MAX,
        };
        let service = self.service.clone();
        let (fetch_remote, fetch_path) = (remote.clone(), path.clone());
        cx.spawn(async move |this, cx| {
            let bytes = service.read_file(fetch_remote, fetch_path, cap).await;
            this.update(cx, |this, cx| this.on_loaded(remote, path, kind, bytes, cx)).ok();
        })
        .detach();
    }

    fn on_loaded(
        &mut self,
        remote: String,
        path: String,
        kind: PreviewKind,
        bytes: Result<Vec<u8>, ServiceError>,
        cx: &mut Context<Self>,
    ) {
        let state = match bytes {
            Err(e) => PreviewState::Error(e.to_string().into()),
            Ok(bytes) => match kind {
                PreviewKind::Image(format) => PreviewState::Image(Arc::new(Image::from_bytes(format, bytes))),
                PreviewKind::Text => PreviewState::Text(String::from_utf8_lossy(&bytes).into_owned().into()),
            },
        };
        if let Some(preview) = self.current.as_mut().filter(|p| p.is(&remote, &path)) {
            preview.state = state.clone();
            cx.notify();
        }
        self.cache_put(format!("{remote}:{path}"), state);
    }

    /// LRU fetch: promote to most-recent on hit.
    fn cache_get(&mut self, key: &str) -> Option<PreviewState> {
        let pos = self.cache.iter().position(|(k, _)| k == key)?;
        let entry = self.cache.remove(pos);
        self.cache.push(entry.clone());
        Some(entry.1)
    }

    fn cache_put(&mut self, key: String, state: PreviewState) {
        if let Some(pos) = self.cache.iter().position(|(k, _)| *k == key) {
            self.cache.remove(pos);
        }
        self.cache.push((key, state));
        if self.cache.len() > CACHE_CAP {
            self.cache.remove(0);
        }
    }

    /// Backend type of the open remote (`RemoteInfo::kind`), or empty if unknown.
    fn open_remote_kind(&self, cx: &App) -> String {
        self.workspace
            .upgrade()
            .map(|ws| {
                let ws = ws.read(cx);
                ws.open_remote
                    .as_deref()
                    .and_then(|name| ws.remotes.iter().find(|r| r.name == name))
                    .map(|r| r.kind.clone())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }

    /// A big type glyph, shown when there's nothing to render (remote / dir /
    /// unsupported). The remote root uses the backend's brand icon.
    fn glyph(&self, entry: &Entry, cx: &App) -> impl IntoElement {
        let icon = match (entry.is_dir, entry.path.is_empty()) {
            (true, true) => remote_icon(&self.open_remote_kind(cx)),
            (true, false) => "icons/folder.svg",
            (false, _) => "icons/file.svg",
        };
        v_flex().flex_1().justify_center().items_center().p_3().child(
            svg().path(icon).size(rem(64.0)).text_color(rgb(FG_SUBTLE)),
        )
    }

    /// Metadata footer: name, size · type, modified, and (for dirs) an on-demand
    /// size row.
    fn info(&self, entry: &Entry, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = match (entry.is_dir, entry.path.is_empty()) {
            (true, true) => match self.open_remote_kind(cx) {
                k if k.is_empty() => "Remote".to_string(),
                k => format!("Remote · {k}"),
            },
            (true, false) => "Folder".to_string(),
            (false, _) => file_kind(&entry.name),
        };
        let size = if entry.is_dir { String::new() } else { human_size(entry.size) };
        let meta = match (size.is_empty(), human_date(&entry.mod_time)) {
            (true, date) => date,
            (false, date) if date.is_empty() => size,
            (false, date) => format!("{size} · {date}"),
        };
        v_flex()
            .flex_shrink_0()
            .gap_1()
            .p_3()
            .border_t_1()
            .border_color(rgb(BORDER_MUTED))
            .child(div().text_color(rgb(FG)).child(entry.name.clone()))
            .child(div().text_xs().text_color(rgb(FG_MUTED)).child(kind))
            .when(!meta.is_empty(), |el| {
                el.child(div().text_xs().text_color(rgb(FG_SUBTLE)).child(meta))
            })
            .when(entry.is_dir, |el| el.child(self.dir_size_row(cx)))
    }

    /// The dir-size affordance: a "Calculate size" link, a spinner, or the result.
    fn dir_size_row(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.current.as_ref().map(|p| &p.dir_size) {
            Some(DirSize::Done(text)) => {
                div().text_xs().text_color(rgb(FG_SUBTLE)).child(text.clone()).into_any_element()
            }
            Some(DirSize::Loading) => h_flex()
                .gap_2()
                .child(spinner("dir-size", px(12.0), FG_MUTED))
                .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child("Calculating…"))
                .into_any_element(),
            _ => text_link("calc-size", "Calculate size", None, |this, _, cx| {
                this.calculate_dir_size(cx)
            }, cx)
            .into_any_element(),
        }
    }
}

/// A synthetic directory entry for the current location — the open folder, or the
/// remote root (empty path) — used as the preview subject with no selection.
fn location_entry(remote: &str, path: &str) -> Entry {
    let name = if path.is_empty() {
        remote.to_string()
    } else {
        path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path).to_string()
    };
    Entry { name, path: path.to_string(), size: 0, mod_time: String::new(), is_dir: true }
}

impl Render for PreviewPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Driven solely by the subject (`current`), never the cursor: with nothing
        // selected the subject is the current directory/remote.
        let content = match self.current.as_ref().map(|p| (&p.entry, &p.state)) {
            None => loading_view().into_any_element(),
            Some((_, PreviewState::Image(image))) => image_view(image.clone()).into_any_element(),
            Some((_, PreviewState::Text(text))) => div()
                .id("preview-text")
                .flex_1()
                .min_h(px(0.0))
                .overflow_scroll()
                .p_3()
                .text_xs()
                .text_color(rgb(FG))
                .child(text.clone())
                .into_any_element(),
            Some((entry, PreviewState::Info)) => self.glyph(entry, cx).into_any_element(),
            Some((_, PreviewState::TooLarge)) => {
                centered("File too large to preview", FG_SUBTLE).into_any_element()
            }
            Some((_, PreviewState::Error(message))) => v_flex()
                .flex_1()
                .justify_center()
                .items_center()
                .p_3()
                .gap_2()
                .child(div().text_xs().text_color(rgb(DANGER)).child(message.clone()))
                .into_any_element(),
            Some((_, PreviewState::Loading)) => loading_view().into_any_element(),
        };
        let entry = self.current.as_ref().map(|p| p.entry.clone());
        v_flex()
            .size_full()
            .min_h(px(0.0))
            .overflow_hidden()
            .child(content)
            .when_some(entry, |el, entry| el.child(self.info(&entry, cx)))
    }
}

impl Workspace {
    pub(crate) fn toggle_preview(&mut self, _: &TogglePreview, _: &mut Window, cx: &mut Context<Self>) {
        // The preview belongs to the file-list view; ignore on the welcome screen.
        if self.open_remote.is_none() {
            return;
        }
        self.toggle_dock(DockPanel::Preview, cx);
    }

    /// Open the preview pane and show the current entry (`set_dock` refreshes it).
    pub(crate) fn open_preview(&mut self, cx: &mut Context<Self>) {
        self.set_dock(Some(DockPanel::Preview), cx);
    }

    pub(crate) fn render_preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .relative()
            .w(self.preview.read(cx).width())
            .min_h(px(0.0))
            .flex_shrink_0()
            .overflow_hidden()
            .bg(rgb(INSET))
            .border_l_1()
            .border_color(rgb(BORDER_MUTED))
            .child(self.resize_handle("preview-resize", ResizeTarget::Preview, cx))
            .child(self.preview.clone())
    }
}
