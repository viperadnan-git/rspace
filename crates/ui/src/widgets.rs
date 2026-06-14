//! Stateless presentation helpers shared across the views.

use std::time::Duration;

use gpui::{
    div, prelude::*, px, rgb, rgba, svg, Animation, AnimationExt as _, AnyView, App, Context, Div,
    Pixels, Render, SharedString, Stateful, Window,
};
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

/// A text button base (padding, rounding, label); caller adds color/hover/click.
pub fn text_button(id: &'static str, label: impl Into<SharedString>) -> Stateful<Div> {
    h_flex().id(id).flex_shrink_0().px_3().py_1().rounded_md().cursor_pointer().child(label.into())
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
    div().text_size(size).text_color(rgb(color)).with_animation(
        id,
        Animation::new(Duration::from_millis(800)).repeat(),
        |el, delta| {
            let i = ((delta * FRAMES.len() as f32) as usize).min(FRAMES.len() - 1);
            el.child(FRAMES[i])
        },
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
pub fn join_path(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
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
