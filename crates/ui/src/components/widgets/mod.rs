//! Stateless presentation helpers shared across the views.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, img, percentage, prelude::*, px, rems, rgb, rgba, svg, Animation, AnimationExt as _,
    AnyView, App, ClickEvent, Context, Div, ElementId, Entity, FocusHandle, FontWeight,
    HighlightStyle, Image, MouseButton, MouseDownEvent, ObjectFit, PathPromptOptions, Pixels, Rems,
    Render, SharedString, Stateful, StyledText, Transformation, Window,
};

use crate::text_input::TextInput;

use crate::theme::*;

mod format;
pub use format::*;

/// A length expressed in rems for a px value at the base rem size, so it scales
/// with the UI zoom. Use for content sizing (icons, control heights, widths);
/// keep `px()` for hairlines and the user-resizable pane widths.
pub fn rem(at_base: f32) -> Rems {
    rems(at_base / BASE_REM)
}

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
    svg().path(path).size(rem(15.0)).flex_shrink_0().text_color(rgb(FG_MUTED))
}

/// Glyph for an rclone backend type, keyed by `RemoteInfo::kind`. Brand icons
/// where available, else a category icon; unknown/new backends fall back to a
/// generic cloud. Add a provider by giving it an arm here.
pub fn remote_icon(kind: &str) -> &'static str {
    match kind {
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
        "qingstor" | "oracleobjectstorage" | "storj" | "sia" | "netstorage" => "icons/database.svg",
        "crypt" => "icons/lock.svg",
        "hasher" => "icons/hasher.svg",
        "compress" => "icons/compress.svg",
        "chunker" => "icons/chunker.svg",
        "union" | "combine" => "icons/union.svg",
        "alias" => "icons/alias.svg",

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

pub fn divider() -> impl IntoElement {
    div().h(px(1.0)).w_full().bg(rgb(BORDER_MUTED))
}

pub fn section_header(label: impl Into<SharedString>) -> Div {
    div().px_3().py_1().text_xs().text_color(rgb(FG_SUBTLE)).child(label.into())
}

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
    if !positions.is_empty() {
        let byte_of: Vec<usize> = text.char_indices().map(|(b, _)| b).chain([text.len()]).collect();
        let style = HighlightStyle {
            color: Some(rgb(hl).into()),
            font_weight: Some(FontWeight::MEDIUM),
            ..Default::default()
        };
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

pub fn icon_button(id: impl Into<gpui::ElementId>, icon: &'static str) -> Stateful<Div> {
    h_flex()
        .id(id)
        .size(rem(22.0))
        .flex_shrink_0()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .text_color(rgb(FG_MUTED))
        .hover(|s| s.bg(rgba(OVERLAY)))
        .child(svg().path(icon).size(rem(14.0)).text_color(rgb(FG_MUTED)))
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

/// `Primary` = accent fill; `Soft` = muted fill; `Secondary` = ghost; `Danger` = red.
pub enum ButtonStyle {
    Primary,
    Soft,
    Secondary,
    Danger,
}

pub struct Button {
    id: &'static str,
    label: SharedString,
    style: ButtonStyle,
    icon: Option<&'static str>,
}

impl Button {
    pub fn new(id: &'static str, label: impl Into<SharedString>, style: ButtonStyle) -> Self {
        Self { id, label: label.into(), style, icon: None }
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
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
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(fg))
            .when_some(self.icon, |b, icon| {
                b.child(svg().path(icon).size(rem(14.0)).flex_shrink_0().text_color(rgb(fg)))
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


pub fn brand_mark() -> impl IntoElement {
    v_flex()
        .items_center()
        .gap_2()
        .child(svg().path("logo.svg").size(rem(56.0)).text_color(rgb(FG)))
        .child(
            div()
                .text_size(rem(20.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(FG))
                .child("rspace"),
        )
}

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
        .children(icon.map(|i| svg().path(i).size(rem(15.0)).flex_shrink_0().text_color(rgb(FG_MUTED))))
        .child(label.into())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| on_click(this, window, cx)))
}

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
        .w(rem(30.0))
        .h(rem(18.0))
        .px(px(2.0))
        .items_center()
        .rounded_full()
        .cursor_pointer()
        .bg(rgb(if on { ACCENT } else { INSET }))
        .border_1()
        .border_color(rgb(if on { ACCENT } else { BORDER_MUTED }))
        .when(on, |el| el.justify_end())
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| on_toggle(this, cx)))
        .child(div().size(rem(12.0)).rounded_full().bg(rgb(if on { 0xffffff } else { FG_MUTED })));
    if let Some(focus) = focus {
        el = el.track_focus(focus).tab_index(0).focus_visible(|s| s.border_color(rgb(ACCENT)));
    }
    el
}

pub fn nav_button(id: &'static str, glyph: &'static str, enabled: bool) -> Stateful<Div> {
    let b = h_flex()
        .id(id)
        .size(rem(24.0))
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

pub fn count_badge(icon: &'static str, color: u32, n: usize) -> impl IntoElement {
    h_flex()
        .gap_1()
        .text_color(rgb(color))
        .child(svg().path(icon).size(rem(13.0)).text_color(rgb(color)))
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

pub fn clamped_width(value: Option<f32>, default: f32, min: f32, max: f32) -> Pixels {
    px(value.unwrap_or(default).clamp(min, max))
}

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
