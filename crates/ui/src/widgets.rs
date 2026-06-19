//! Stateless presentation helpers shared across the views.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, img, percentage, prelude::*, px, rgb, rgba, svg, Animation, AnimationExt as _, AnyView,
    App, ClickEvent, Context, Div, ElementId, Entity, FocusHandle, FontWeight, HighlightStyle,
    Image, MouseButton, MouseDownEvent, ObjectFit, PathPromptOptions, Pixels, Render, SharedString,
    Stateful, StyledText, Transformation, Window,
};

use crate::text_input::TextInput;
use rspace_core::{SortField, SortOrder};
use rspace_rclone_rc::Entry;

use crate::theme::*;

/// Hover tooltip surface, built via [`tooltip_text`].
pub struct Tooltip {
    text: SharedString,
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w_96()
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

pub fn tooltip_text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let text = text.into();
    move |_window, cx| cx.new(|_| Tooltip { text: text.clone() }).into()
}

pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

pub fn v_flex() -> Div {
    div().flex().flex_col()
}

pub fn file_icon(is_dir: bool) -> impl IntoElement {
    let path = if is_dir { "icons/folder.svg" } else { "icons/file.svg" };
    svg().path(path).size(px(15.0)).flex_shrink_0().text_color(rgb(FG_MUTED))
}

/// Glyph for an rclone backend type, keyed by `RemoteInfo::kind`. Brand icons
/// where available, else a category icon; unknown/new backends fall back to a
/// generic cloud. Add a provider by giving it an arm here.
pub fn remote_icon(kind: &str) -> &'static str {
    match kind {
        // Brand icons.
        "drive" => "icons/drive.svg",
        "dropbox" => "icons/dropbox.svg",
        "googlecloudstorage" => "icons/gcs.svg",
        "b2" => "icons/b2.svg",
        "box" => "icons/box.svg",
        "mega" => "icons/mega.svg",
        "swift" => "icons/swift.svg",
        "yandex" => "icons/yandex.svg",
        "protondrive" => "icons/protondrive.svg",
        "iclouddrive" => "icons/icloud.svg",
        "onedrive" => "icons/onedrive.svg",
        "s3" => "icons/s3.svg",
        "azureblob" | "azurefiles" => "icons/azureblob.svg",
        "googlephotos" => "icons/googlephotos.svg",
        "internetarchive" => "icons/internetarchive.svg",
        "zoho" => "icons/zoho.svg",
        "seafile" => "icons/seafile.svg",
        "mailru" => "icons/mailru.svg",
        "sharefile" => "icons/sharefile.svg",
        "smb" => "icons/smb.svg",
        "pixeldrain" => "icons/image.svg",

        // Local disk / in-process.
        "local" => "icons/hard_drive.svg",
        "memory" => "icons/memory.svg",
        "cache" => "icons/cache.svg",
        // Network protocols (generic WebDAV is just a protocol, not Nextcloud).
        "sftp" | "ftp" | "http" | "hdfs" | "nfs" | "webdav" => "icons/server.svg",
        "nextcloud" => "icons/nextcloud.svg",
        "owncloud" => "icons/owncloud.svg",
        // Object storage (S3-compatible and friends).
        "qingstor" | "oracleobjectstorage" | "storj" | "sia" | "netstorage" => "icons/database.svg",
        // Wrapper / composite backends.
        "crypt" => "icons/lock.svg",
        "hasher" => "icons/hasher.svg",
        "compress" => "icons/compress.svg",
        "chunker" => "icons/chunker.svg",
        "union" | "combine" => "icons/union.svg",
        "alias" => "icons/alias.svg",

        // Everything else (other consumer clouds, new backends).
        _ => "icons/cloud.svg",
    }
}

pub fn list_item(id: usize, selected: bool, focused: bool) -> Stateful<Div> {
    let base = h_flex().id(id).w_full().justify_between().gap_2().px_3().py_1().cursor_pointer();
    if selected && focused {
        base.bg(rgba(SELECT))
    } else if selected {
        base.bg(rgba(SELECT_MUTED))
    } else {
        base.hover(|s| s.bg(rgba(OVERLAY)))
    }
}

/// A settings-sidebar nav entry: an inset, rounded row with a neutral fill on
/// selection and a subtle hover (matching Zed's settings navbar). The container
/// supplies the horizontal inset so the rounded highlight floats off the edges.
pub fn nav_item(id: usize, selected: bool, focused: bool) -> Stateful<Div> {
    let base = h_flex().id(id).w_full().gap_2().items_center().px_2().py_1().rounded_md().cursor_pointer();
    if selected && focused {
        base.bg(rgba(SELECT))
    } else if selected {
        base.bg(rgba(SELECT_MUTED))
    } else {
        base.hover(|s| s.bg(rgba(OVERLAY)))
    }
}

/// A full-width hairline separator.
pub fn divider() -> impl IntoElement {
    div().h(px(1.0)).w_full().bg(rgb(BORDER_MUTED))
}

/// A small uppercase section label (e.g. "RECENT").
pub fn section_header(label: impl Into<SharedString>) -> Div {
    div().px_3().py_1().text_xs().text_color(rgb(FG_SUBTLE)).child(label.into())
}

/// A picker/command-menu row: Zed-style inset selection pill (rounded, off the
/// card edge). Caller fills the content (label left, key binding right).
pub fn picker_item(id: usize, selected: bool) -> Stateful<Div> {
    let base = h_flex().id(id).w_full().justify_between().gap_2().px(px(6.0)).py_1().rounded_md().cursor_pointer();
    if selected {
        base.bg(rgba(SELECT))
    } else {
        base.hover(|s| s.bg(rgba(OVERLAY)))
    }
}

/// A centered image scaled to fit its box: shown at natural size, scaled *down*
/// (never up) to fit, preserving aspect, and clipped so it can't overflow. The
/// box must be size-bounded by the caller (e.g. `flex_1().min_h_0()`).
pub fn image_view(image: Arc<Image>) -> impl IntoElement {
    div()
        .flex_1()
        .min_h(px(0.0))
        .w_full()
        .overflow_hidden()
        .flex()
        .justify_center()
        .items_center()
        .child(
            img(image)
                .id("preview-image")
                .max_w_full()
                .max_h_full()
                .object_fit(ObjectFit::Contain)
                .with_fallback(|| centered("Can't preview this image", FG_SUBTLE).into_any_element()),
        )
}

/// A single-line label with fuzzy-matched chars (by char index, ascending)
/// emphasized in `hl`; the rest in `base`. One text element so it truncates with
/// an ellipsis when the row is too narrow (grows/shrinks in its flex parent).
pub fn highlighted_label(text: &str, positions: &[usize], base: u32, hl: u32) -> impl IntoElement {
    let mut styled = StyledText::new(text.to_string());
    // Skip the char→byte mapping entirely for the common unmatched case.
    if !positions.is_empty() {
        let byte_of: Vec<usize> = text.char_indices().map(|(b, _)| b).chain([text.len()]).collect();
        let style = HighlightStyle {
            color: Some(rgb(hl).into()),
            font_weight: Some(FontWeight::MEDIUM),
            ..Default::default()
        };
        // Char indices → byte ranges, merging consecutive matched chars into runs.
        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
        for &ci in positions {
            if ci + 1 >= byte_of.len() {
                continue;
            }
            let (start, end) = (byte_of[ci], byte_of[ci + 1]);
            match highlights.last_mut() {
                Some((r, _)) if r.end == start => r.end = end,
                _ => highlights.push((start..end, style)),
            }
        }
        styled = styled.with_highlights(highlights);
    }
    div().flex_1().min_w(px(0.0)).truncate().text_color(rgb(base)).child(styled)
}

/// A keyboard-shortcut chip (e.g. the binding shown on a command row).
pub fn key_binding(keys: impl Into<SharedString>) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .px(px(6.0))
        .rounded_sm()
        .bg(rgba(OVERLAY))
        .text_xs()
        .text_color(rgb(FG_SUBTLE))
        .child(keys.into())
}

/// A square icon button: muted svg glyph, rounded hover background.
pub fn icon_button(id: impl Into<gpui::ElementId>, icon: &'static str) -> Stateful<Div> {
    h_flex()
        .id(id)
        .size(px(22.0))
        .flex_shrink_0()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .text_color(rgb(FG_MUTED))
        .hover(|s| s.bg(rgba(OVERLAY)))
        .child(svg().path(icon).size(px(14.0)).text_color(rgb(FG_MUTED)))
}

/// Base for a centered modal card: elevated surface that swallows clicks so they
/// don't fall through to the dismiss-on-click backdrop.
pub fn modal_card<V: 'static>(id: &'static str, focus: &FocusHandle, cx: &mut Context<V>) -> Stateful<Div> {
    let focus = focus.clone();
    v_flex()
        .id(id)
        .p_5()
        .rounded_lg()
        .bg(rgb(ELEVATED))
        .border_1()
        .border_color(rgb(BORDER_MUTED))
        .shadow_lg()
        // A click on the card itself (inputs stop propagation, so never them)
        // blurs the focused field and keeps focus/shortcuts on the card. Also
        // stops the click reaching any backdrop behind the modal.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _: &MouseDownEvent, window, cx| {
                focus.focus(window, cx);
                cx.stop_propagation();
            }),
        )
}

/// Focus `handle` once, tracked by `done`: a container takes initial keyboard
/// focus on open without stealing it back from child inputs each frame.
pub fn focus_once(done: &mut bool, handle: &FocusHandle, window: &mut Window, cx: &mut App) {
    if !*done {
        *done = true;
        handle.focus(window, cx);
    }
}

/// Prompt for a single file and write its path into `input`; no-op on cancel.
pub fn pick_file_into<V: 'static>(input: Entity<TextInput>, cx: &mut Context<V>) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |this, cx| {
        if let Ok(Ok(Some(paths))) = rx.await {
            if let Some(p) = paths.into_iter().next() {
                let text = p.to_string_lossy().into_owned();
                this.update(cx, |_, cx| {
                    input.update(cx, |i, cx| i.set_text(text, cx));
                    cx.notify();
                })
                .ok();
            }
        }
    })
    .detach();
}

/// Transparent border that turns accent on keyboard focus (Zed-style focus ring).
pub fn focus_ring<E: Styled + InteractiveElement>(el: E) -> E {
    el.border_1().border_color(rgba(0x0000_0000)).focus_visible(|s| s.border_color(rgb(ACCENT)))
}

/// Visual variants of [`button`]. `Primary` = accent fill; `Soft` = muted fill
/// (Choose…/Clean up/Check again); `Secondary` = ghost (no fill); `Danger` = red.
pub enum ButtonStyle {
    Primary,
    Soft,
    Secondary,
    Danger,
}

/// A labelled action button in one of the [`ButtonStyle`] variants.
/// A text button with an optional leading icon and an optional fixed height.
/// [`button`] is the common (label-only, default-height) shorthand.
pub struct Button {
    id: &'static str,
    label: SharedString,
    style: ButtonStyle,
    icon: Option<&'static str>,
    height: Option<Pixels>,
}

impl Button {
    pub fn new(id: &'static str, label: impl Into<SharedString>, style: ButtonStyle) -> Self {
        Self { id, label: label.into(), style, icon: None, height: None }
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = Some(height);
        self
    }

    pub fn build<V: 'static>(
        self,
        on_click: impl Fn(&mut V, &mut Context<V>) + 'static,
        cx: &mut Context<V>,
    ) -> Stateful<Div> {
        // svg colour doesn't inherit, so match the label colour explicitly.
        let fg = match self.style {
            ButtonStyle::Primary | ButtonStyle::Danger => 0xffffff,
            _ => FG,
        };
        let base = h_flex()
            .id(self.id)
            .flex_shrink_0()
            .gap_1p5()
            .items_center()
            .px_3()
            .when_some(self.height, |b, h| b.h(h))
            .when(self.height.is_none(), |b| b.py_1())
            .rounded_md()
            .cursor_pointer()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(fg))
            .when_some(self.icon, |b, icon| {
                b.child(svg().path(icon).size(px(14.0)).flex_shrink_0().text_color(rgb(fg)))
            })
            .child(self.label)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| on_click(this, cx)));
        match self.style {
            ButtonStyle::Primary => base.bg(rgb(ACCENT)).hover(|s| s.bg(rgb(ACCENT_HOVER))),
            ButtonStyle::Soft => base.bg(rgba(OVERLAY)).hover(|s| s.bg(rgba(SELECT_MUTED))),
            ButtonStyle::Secondary => base.hover(|s| s.bg(rgba(OVERLAY))),
            ButtonStyle::Danger => base.bg(rgb(DANGER)),
        }
    }
}


/// The rspace brand mark: the logo over the wordmark, as shown on the home
/// (welcome) screen.
pub fn brand_mark() -> impl IntoElement {
    v_flex()
        .items_center()
        .gap_2()
        .child(svg().path("logo.svg").size(px(56.0)).text_color(rgb(FG)))
        .child(
            div()
                .text_size(px(20.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(FG))
                .child("rspace"),
        )
}

/// An inline text link with an optional leading icon: muted, brightening to
/// accent on hover. The click receives the window (e.g. to move focus).
pub fn text_link<V: 'static>(
    id: &'static str,
    label: impl Into<SharedString>,
    icon: Option<&'static str>,
    on_click: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    cx: &mut Context<V>,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .gap_1p5()
        .items_center()
        .text_sm()
        .cursor_pointer()
        .text_color(rgb(FG_MUTED))
        .hover(|s| s.text_color(rgb(ACCENT)))
        .children(icon.map(|i| svg().path(i).size(px(15.0)).flex_shrink_0().text_color(rgb(FG_MUTED))))
        .child(label.into())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| on_click(this, window, cx)))
}

/// A small selectable pill (example values, segmented presets).
pub fn chip(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Stateful<Div> {
    let base = h_flex()
        .id(id)
        .px_2()
        .py(px(3.0))
        .rounded_md()
        .cursor_pointer()
        .text_xs()
        .border_1()
        .child(label.into());
    if selected {
        base.bg(rgba(ACCENT_SOFT)).border_color(rgb(ACCENT)).text_color(rgb(FG))
    } else {
        base.bg(rgb(ELEVATED))
            .border_color(rgb(BORDER_MUTED))
            .text_color(rgb(FG_MUTED))
            .hover(|s| s.border_color(rgb(FG_SUBTLE)).text_color(rgb(FG)))
    }
}

/// A compact Zed-style switch; focusable (Tab-reachable) when given a handle,
/// toggled by click or Enter/Space (gpui synthesizes the click on key press).
pub fn switch<V: 'static>(
    id: impl Into<ElementId>,
    on: bool,
    focus: Option<&FocusHandle>,
    on_toggle: impl Fn(&mut V, &mut Context<V>) + 'static,
    cx: &mut Context<V>,
) -> Stateful<Div> {
    let mut el = h_flex()
        .id(id)
        .w(px(30.0))
        .h(px(18.0))
        .px(px(2.0))
        .items_center()
        .rounded_full()
        .cursor_pointer()
        .bg(rgb(if on { ACCENT } else { INSET }))
        .border_1()
        .border_color(rgb(if on { ACCENT } else { BORDER_MUTED }))
        .when(on, |el| el.justify_end())
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| on_toggle(this, cx)))
        .child(div().size(px(12.0)).rounded_full().bg(rgb(if on { 0xffffff } else { FG_MUTED })));
    if let Some(focus) = focus {
        el = el.track_focus(focus).tab_index(0).focus_visible(|s| s.border_color(rgb(ACCENT)));
    }
    el
}

pub fn nav_button(id: &'static str, glyph: &'static str, enabled: bool) -> Stateful<Div> {
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

/// A labeled setting: title, description, and its control.
pub fn setting_block(title: &str, desc: &str, control: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_2()
        .child(div().text_sm().text_color(rgb(FG)).child(title.to_string()))
        .child(div().text_xs().text_color(rgb(FG_MUTED)).child(desc.to_string()))
        .child(control)
}

pub fn info_row(label: &str, value: &str) -> impl IntoElement {
    h_flex()
        .w_full()
        .justify_between()
        .gap_4()
        .text_xs()
        .child(div().flex_shrink_0().text_color(rgb(FG_MUTED)).child(label.to_string()))
        .child(div().min_w(px(0.0)).truncate().text_color(rgb(FG_SUBTLE)).child(value.to_string()))
}

/// An icon + count chip, used for the status-bar job tallies.
pub fn count_badge(icon: &'static str, color: u32, n: usize) -> impl IntoElement {
    h_flex()
        .gap_1()
        .text_color(rgb(color))
        .child(svg().path(icon).size(px(13.0)).text_color(rgb(color)))
        .child(n.to_string())
}

pub fn centered(text: &'static str, color: u32) -> Div {
    v_flex().size_full().justify_center().items_center().text_color(rgb(color)).child(text)
}

pub fn spinner(id: impl Into<gpui::ElementId>, size: Pixels, color: u32) -> impl IntoElement {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    // Tight line height + no shrink: an inline indicator that never grows its row.
    div().text_size(size).line_height(size).flex_shrink_0().text_color(rgb(color)).with_animation(
        id,
        Animation::new(Duration::from_millis(800)).repeat(),
        |el, delta| {
            let i = ((delta * FRAMES.len() as f32) as usize).min(FRAMES.len() - 1);
            el.child(FRAMES[i])
        },
    )
}

/// A continuously rotating icon — inline activity indicator for running jobs.
pub fn spinner_icon(
    id: impl Into<gpui::ElementId>,
    icon: &'static str,
    size: Pixels,
    color: u32,
) -> impl IntoElement {
    svg().path(icon).size(size).flex_shrink_0().text_color(rgb(color)).with_animation(
        id,
        Animation::new(Duration::from_millis(900)).repeat(),
        |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
    )
}

pub fn loading_view() -> impl IntoElement {
    v_flex()
        .size_full()
        .justify_center()
        .items_center()
        .gap_3()
        .child(spinner("panel-spinner", px(28.0), ACCENT))
        .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child("Loading…"))
}

pub fn sort_arrow(order: SortOrder) -> &'static str {
    match order {
        SortOrder::Asc => "↑",
        SortOrder::Desc => "↓",
    }
}

/// Directories first, then by `field`/`order` within each group.
pub fn sort_entries(entries: &mut [Entry], field: SortField, order: SortOrder) {
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

/// `rclone <verb> "<arg>" …` — the CLI shown by a task row's copy button.
pub fn rclone_cmd(verb: &str, args: &[&str]) -> String {
    let mut s = format!("rclone {verb}");
    for a in args {
        s.push_str(&format!(" \"{a}\""));
    }
    s
}

pub fn human_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let s = ms / 1000;
        format!("{}m{:02}s", s / 60, s % 60)
    }
}

/// Parent of a `/`-separated remote path (empty at the root).
pub fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// Join `name` under `dir`, avoiding a leading slash at the root.
// Single source in rclone_rc::ops (the lower crate) so path-joining can't diverge.
pub use rspace_rclone_rc::join as join_path;

/// Best-effort `Mon D, YYYY  HH:MM` from rclone's RFC3339 mod time (UTC).
pub fn human_date(rfc3339: &str) -> String {
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    if rfc3339.len() < 16 {
        return String::new();
    }
    let (date, time) = (&rfc3339[..10], &rfc3339[11..16]);
    let p: Vec<&str> = date.split('-').collect();
    let (Some(y), Some(m), Some(d)) = (p.first(), p.get(1).and_then(|s| s.parse::<usize>().ok()), p.get(2))
    else {
        return String::new();
    };
    let mon = MONTHS.get(m.wrapping_sub(1)).copied().unwrap_or("");
    format!("{mon} {}, {y}  {time}", d.trim_start_matches('0'))
}

/// A persisted layout width: the stored value or `default`, clamped to bounds.
pub fn clamped_width(value: Option<f32>, default: f32, min: f32, max: f32) -> Pixels {
    px(value.unwrap_or(default).clamp(min, max))
}

/// A labeled form field: title (+ required marker), optional help, then the
/// control. Reused by any form so fields render consistently.
pub fn form_field(label: &str, help: &str, required: bool, control: impl IntoElement) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .gap_1()
                .child(div().text_sm().text_color(rgb(FG)).child(label.to_string()))
                .when(required, |el| el.child(div().text_color(rgb(DANGER)).child("*"))),
        )
        .when(!help.is_empty(), |el| {
            el.child(div().text_xs().text_color(rgb(FG_SUBTLE)).child(help.to_string()))
        })
        .child(control)
}

/// Friendly type label for the preview info card, e.g. `PNG` / `RS` / `File`.
pub fn file_kind(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext.to_ascii_uppercase(),
        _ => "File".to_string(),
    }
}

pub fn human_size(bytes: i64) -> String {
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
