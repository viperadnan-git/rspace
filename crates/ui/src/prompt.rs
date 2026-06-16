//! An inline text-edit entity for the explorer list (new folder / rename). It
//! emits the entered text on submit; the action to run lives in the opener's
//! subscription, keeping the entity presentational.

use gpui::{Entity, EventEmitter};

use super::*;
use crate::text_input::TextInput;

pub(crate) enum PromptEvent {
    Submitted(String),
    Cancelled,
}

pub(crate) struct PromptModal {
    input: Entity<TextInput>,
    icon_dir: bool,
    /// Path of the entry being renamed, or `None` for a new item at the top.
    pub(crate) target: Option<String>,
    autofocus: bool,
}

impl EventEmitter<PromptEvent> for PromptModal {}

impl PromptModal {
    pub(crate) fn new(
        value: impl Into<String>,
        placeholder: impl Into<SharedString>,
        icon_dir: bool,
        target: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let value = value.into();
        let input = cx.new(|cx| {
            let mut input = TextInput::new(cx, placeholder).bare();
            if !value.is_empty() {
                input.set_text(value, cx);
            }
            input
        });
        Self { input, icon_dir, target, autofocus: true }
    }

    fn submit(&mut self, _: &PromptSubmit, _: &mut Window, cx: &mut Context<Self>) {
        let value = self.input.read(cx).text().trim().to_string();
        // Empty input: stay open rather than silently doing nothing.
        if !value.is_empty() {
            cx.emit(PromptEvent::Submitted(value));
        }
    }

    fn cancel(&mut self, _: &PromptCancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(PromptEvent::Cancelled);
    }
}

impl Render for PromptModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.autofocus {
            self.autofocus = false;
            self.input.read(cx).focus_handle(cx).focus(window, cx);
        }
        h_flex()
            .key_context("modal Prompt")
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::cancel))
            .w_full()
            .gap_2()
            .px_3()
            .py_1()
            .items_center()
            .bg(rgba(SELECT))
            .border_1()
            .border_color(rgb(ACCENT))
            .child(file_icon(self.icon_dir))
            .child(div().flex_grow(1.0).min_w(px(0.0)).child(self.input.clone()))
    }
}
