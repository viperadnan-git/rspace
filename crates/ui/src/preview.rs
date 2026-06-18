//! File-preview pane: classify by extension, fetch over `--rc-serve` (capped,
//! cached), and render an image / text view, or a rich info card otherwise.
//!
//! Adding a format: give it an arm in [`classify`] and a render branch in
//! [`Workspace::render_preview`]. Adding a whole new kind: add a [`PreviewKind`]
//! variant (drives the fetch) and a [`PreviewState`] variant (drives rendering).

use std::sync::Arc;

use gpui::{Image, ImageFormat};

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
pub(crate) enum DirSize {
    Idle,
    Loading,
    Done(String),
}

/// The preview subject: a selected entry, or the current directory/remote when
/// nothing is selected. Identity is `(remote, entry.path)` — qualified by remote
/// so an async result can't bind to a same-named path on a different remote.
pub(crate) struct Preview {
    pub remote: String,
    pub entry: Entry,
    pub state: PreviewState,
    pub dir_size: DirSize,
}

impl Preview {
    fn is(&self, remote: &str, path: &str) -> bool {
        self.remote == remote && self.entry.path == path
    }
}

impl Workspace {
    pub(crate) fn toggle_preview(&mut self, _: &TogglePreview, _: &mut Window, cx: &mut Context<Self>) {
        // The preview belongs to the file-list view; ignore the toggle on the
        // welcome screen (no remote open).
        if self.open_remote.is_none() {
            return;
        }
        self.preview_open = !self.preview_open;
        self.ui.preview_open = self.preview_open;
        self.save_ui();
        self.refresh_preview(cx);
        cx.notify();
    }

    /// Open the preview pane (persisting the layout) and show the current entry.
    pub(crate) fn open_preview(&mut self, cx: &mut Context<Self>) {
        if !self.preview_open {
            self.preview_open = true;
            self.ui.preview_open = true;
            self.save_ui();
        }
        self.refresh_preview(cx);
        cx.notify();
    }

    /// Keep the preview subject in sync: the selected entry, or the current
    /// directory/remote when nothing is selected (the cursor is not a selection).
    /// Cheap no-op when the pane is closed or already showing the subject.
    pub(crate) fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        if !self.preview_open {
            return;
        }
        let Some(remote) = self.open_remote.as_deref() else {
            self.preview = None;
            return;
        };
        // Cheap identity from borrows — no allocation on the steady-state no-op
        // (this runs on every render while the pane is open).
        let subject_path = if self.selected.is_empty() {
            self.path.as_str()
        } else {
            self.entries().get(self.entry_sel).map_or(self.path.as_str(), |e| e.path.as_str())
        };
        if self.preview.as_ref().is_some_and(|p| p.is(remote, subject_path)) {
            return;
        }
        let remote = remote.to_string();
        let entry = if self.selected.is_empty() {
            self.location_entry()
        } else {
            self.entries().get(self.entry_sel).cloned().unwrap_or_else(|| self.location_entry())
        };
        let key = format!("{remote}:{}", entry.path);
        if let Some(state) = self.preview_cache_get(&key) {
            self.preview = Some(Preview { remote, entry, state, dir_size: DirSize::Idle });
            return;
        }
        let state = match classify(&entry.name) {
            _ if entry.is_dir => PreviewState::Info,
            None => PreviewState::Info,
            Some(PreviewKind::Image(_)) if entry.size as u64 > IMAGE_MAX => PreviewState::TooLarge,
            Some(kind) => {
                self.spawn_preview_fetch(remote.clone(), entry.path.clone(), kind, cx);
                PreviewState::Loading
            }
        };
        self.preview = Some(Preview { remote, entry, state, dir_size: DirSize::Idle });
    }

    /// A synthetic directory entry for the current location — the open folder, or
    /// the remote root (empty path) — used as the preview subject with no
    /// selection.
    fn location_entry(&self) -> Entry {
        let name = if self.path.is_empty() {
            self.open_remote.clone().unwrap_or_default()
        } else {
            self.path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(self.path.as_str()).to_string()
        };
        Entry { name, path: self.path.clone(), size: 0, mod_time: String::new(), is_dir: true }
    }

    /// Walk the selected directory to total its size (rclone `operations/size`),
    /// shown inline in the info card. Opt-in since it can be expensive.
    fn calculate_dir_size(&mut self, cx: &mut Context<Self>) {
        let (Some(remote), Some(preview)) = (self.open_remote.clone(), self.preview.as_mut()) else {
            return;
        };
        let path = preview.entry.path.clone();
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
                if let Some(preview) = this.preview.as_mut().filter(|p| p.is(&remote, &path)) {
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

    fn spawn_preview_fetch(
        &self,
        remote: String,
        path: String,
        kind: PreviewKind,
        cx: &mut Context<Self>,
    ) {
        let cap = match kind {
            PreviewKind::Image(_) => IMAGE_MAX,
            PreviewKind::Text => TEXT_MAX,
        };
        let service = self.service.clone();
        let (fetch_remote, fetch_path) = (remote.clone(), path.clone());
        cx.spawn(async move |this, cx| {
            let bytes = service.read_file(fetch_remote, fetch_path, cap).await;
            this.update(cx, |this, cx| this.on_preview_loaded(remote, path, kind, bytes, cx)).ok();
        })
        .detach();
    }

    fn on_preview_loaded(
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
        if self.preview_open {
            if let Some(preview) = self.preview.as_mut().filter(|p| p.is(&remote, &path)) {
                preview.state = state.clone();
                cx.notify();
            }
        }
        self.preview_cache_put(format!("{remote}:{path}"), state);
    }

    /// LRU fetch: promote to most-recent on hit.
    fn preview_cache_get(&mut self, key: &str) -> Option<PreviewState> {
        let pos = self.preview_cache.iter().position(|(k, _)| k == key)?;
        let entry = self.preview_cache.remove(pos);
        self.preview_cache.push(entry.clone());
        Some(entry.1)
    }

    fn preview_cache_put(&mut self, key: String, state: PreviewState) {
        if let Some(pos) = self.preview_cache.iter().position(|(k, _)| *k == key) {
            self.preview_cache.remove(pos);
        }
        self.preview_cache.push((key, state));
        if self.preview_cache.len() > CACHE_CAP {
            self.preview_cache.remove(0);
        }
    }

    pub(crate) fn render_preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Driven solely by the subject (`self.preview`), never the cursor: with
        // nothing selected the subject is the current directory/remote.
        let content = match self.preview.as_ref().map(|p| (&p.entry, &p.state)) {
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
            Some((entry, PreviewState::Info)) => self.preview_glyph(entry).into_any_element(),
            Some((_, PreviewState::TooLarge)) => {
                centered("File too large to preview", FG_SUBTLE).into_any_element()
            }
            Some((_, PreviewState::Error(message))) => {
                v_flex().flex_1().justify_center().items_center().p_3().gap_2().child(
                    div().text_xs().text_color(rgb(DANGER)).child(message.clone()),
                ).into_any_element()
            }
            Some((_, PreviewState::Loading)) => loading_view().into_any_element(),
        };

        v_flex()
            .relative()
            .w(self.preview_width)
            .min_h(px(0.0))
            .flex_shrink_0()
            .overflow_hidden()
            .bg(rgb(INSET))
            .border_l_1()
            .border_color(rgb(BORDER_MUTED))
            .child(self.resize_handle("preview-resize", ResizeTarget::Preview, PREVIEW_W, cx))
            .child(content)
            .when_some(self.preview.as_ref().map(|p| p.entry.clone()), |el, entry| {
                el.child(self.preview_info(&entry, cx))
            })
    }

    /// Backend type of the open remote (`RemoteInfo::kind`), or empty if unknown.
    fn open_remote_kind(&self) -> String {
        self.open_remote
            .as_deref()
            .and_then(|name| self.remotes.iter().find(|r| r.name == name))
            .map(|r| r.kind.clone())
            .unwrap_or_default()
    }

    /// A big type glyph, shown when there's nothing to render (remote / dir /
    /// unsupported). The remote root uses the backend's brand icon.
    fn preview_glyph(&self, entry: &Entry) -> impl IntoElement {
        let icon = match (entry.is_dir, entry.path.is_empty()) {
            (true, true) => remote_icon(&self.open_remote_kind()),
            (true, false) => "icons/folder.svg",
            (false, _) => "icons/file.svg",
        };
        v_flex().flex_1().justify_center().items_center().p_3().child(
            svg().path(icon).size(px(64.0)).text_color(rgb(FG_SUBTLE)),
        )
    }

    /// Metadata footer: name, size · type, modified, and (for dirs) an on-demand
    /// size row.
    fn preview_info(&self, entry: &Entry, cx: &mut Context<Self>) -> impl IntoElement {
        // The location subject at the remote root (empty path) is the remote
        // itself — label it with its backend type.
        let kind = match (entry.is_dir, entry.path.is_empty()) {
            (true, true) => match self.open_remote_kind() {
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

    /// The dir-size affordance: a "Calculate size" link, a spinner while walking,
    /// or the result.
    fn dir_size_row(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.preview.as_ref().map(|p| &p.dir_size) {
            Some(DirSize::Done(text)) => {
                div().text_xs().text_color(rgb(FG_SUBTLE)).child(text.clone()).into_any_element()
            }
            Some(DirSize::Loading) => h_flex()
                .gap_2()
                .child(spinner("dir-size", px(12.0), FG_MUTED))
                .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child("Calculating…"))
                .into_any_element(),
            _ => div()
                .id("calc-size")
                .text_xs()
                .text_color(rgb(ACCENT))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(ACCENT_HOVER)))
                .child("Calculate size")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.calculate_dir_size(cx)))
                .into_any_element(),
        }
    }
}
