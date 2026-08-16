//! The keyboard-shortcuts reference: a dismissable modal listing every bound
//! command, grouped by category. Purely presentational — it reads the single
//! source of truth in [`crate::keymap`], so it can never drift from what's bound.

use gpui::{EventEmitter, FocusHandle, Focusable};

use super::*;
use crate::keymap::{commands, Category, Command};

actions!(keybindings, [DismissKeybindings]);

pub(crate) struct KeybindingsView {
    focus_handle: FocusHandle,
    focused: bool,
}

impl EventEmitter<DismissEvent> for KeybindingsView {}

impl Focusable for KeybindingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl KeybindingsView {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        Self { focus_handle: cx.focus_handle(), focused: false }
    }

    fn dismiss(&mut self, _: &DismissKeybindings, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

/// Render the keystrokes of one command as chips, e.g. `⌘C` or `↓ J`. Empty for
/// palette-only commands (filtered out before this is called).
fn keys_for(command: &Command) -> impl IntoElement {
    h_flex().flex_shrink_0().gap_1().children(command.bindings.iter().map(|b| {
        let keys = b.keystrokes().iter().map(ToString::to_string).collect::<Vec<_>>().join(" ");
        key_binding(keys)
    }))
}

fn group(category: Category, commands: &[Command]) -> Option<impl IntoElement> {
    let rows: Vec<_> = commands
        .iter()
        .filter(|c| c.category == category && !c.bindings.is_empty())
        .collect();
    if rows.is_empty() {
        return None;
    }
    Some(
        v_flex().w_full().child(section_header(category.title())).children(rows.into_iter().map(|c| {
            h_flex()
                .w_full()
                .py_1()
                .px_3()
                .gap_3()
                .justify_between()
                .items_center()
                .child(div().flex_1().min_w(px(0.0)).truncate().text_color(rgb(FG)).child(c.label))
                .child(keys_for(c))
        })),
    )
}

impl Render for KeybindingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        focus_once(&mut self.focused, &self.focus_handle, window, cx);
        let commands = commands();
        modal_card("keybindings-card", &self.focus_handle, cx)
            .key_context("modal Keybindings")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::dismiss))
            .w(rem(460.0))
            .max_h(rem(560.0))
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(div().text_lg().text_color(rgb(FG)).child("Keyboard Shortcuts"))
                    .child(
                        icon_button("keybindings-close", "icons/x.svg")
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(DismissEvent))),
                    ),
            )
            .child(
                div()
                    .id("keybindings-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .gap_3()
                            .children(Category::ORDER.into_iter().filter_map(|cat| group(cat, &commands))),
                    ),
            )
    }
}
