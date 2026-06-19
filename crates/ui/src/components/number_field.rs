//! A compact numeric stepper: `[−] value [+]` with a typeable value, clamped to
//! a range. Emits [`NumberFieldEvent::Changed`]; the owner applies it.

use gpui::{ClickEvent, Entity, EventEmitter, FocusHandle, Focusable};

use super::*;

actions!(number_field, [NumberCommit]);
use crate::text_input::TextInput;

pub(crate) enum NumberFieldEvent {
    Changed(u64),
}

pub(crate) struct NumberField {
    focus_handle: FocusHandle,
    input: Entity<TextInput>,
    value: u64,
    min: u64,
    max: u64,
    step: u64,
}

impl EventEmitter<NumberFieldEvent> for NumberField {}

impl Focusable for NumberField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl NumberField {
    pub(crate) fn new(value: u64, min: u64, max: u64, step: u64, cx: &mut Context<Self>) -> Self {
        let value = value.clamp(min, max);
        let input = cx.new(|cx| TextInput::new(cx, "").bare().centered());
        input.update(cx, |i, cx| i.set_text(value.to_string(), cx));
        Self { focus_handle: cx.focus_handle(), input, value, min, max, step }
    }

    /// The current typed value (falls back to the last committed value), clamped.
    fn typed(&self, cx: &App) -> u64 {
        self.input
            .read(cx)
            .text()
            .trim()
            .parse::<u64>()
            .unwrap_or(self.value)
            .clamp(self.min, self.max)
    }

    fn set(&mut self, value: u64, cx: &mut Context<Self>) {
        let value = value.clamp(self.min, self.max);
        self.value = value;
        self.input.update(cx, |i, cx| i.set_text(value.to_string(), cx));
        cx.emit(NumberFieldEvent::Changed(value));
        cx.notify();
    }

    fn step_by(&mut self, delta: i64, cx: &mut Context<Self>) {
        let next = (self.typed(cx) as i64 + delta).clamp(self.min as i64, self.max as i64) as u64;
        self.set(next, cx);
    }

    fn commit(&mut self, _: &NumberCommit, _: &mut Window, cx: &mut Context<Self>) {
        let value = self.typed(cx);
        self.set(value, cx);
    }

    fn stepper(&self, id: &'static str, glyph: &'static str, delta: i64, cx: &mut Context<Self>) -> Stateful<Div> {
        h_flex()
            .id(id)
            .size(px(24.0))
            .justify_center()
            .items_center()
            .cursor_pointer()
            .text_color(rgb(FG_MUTED))
            .hover(|s| s.text_color(rgb(FG)).bg(rgba(OVERLAY)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.step_by(delta, cx)))
            .child(glyph)
    }
}

impl Render for NumberField {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rule = || div().w(px(1.0)).h_full().bg(rgb(BORDER_MUTED));
        h_flex()
            .key_context("NumberField")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::commit))
            .h(px(26.0))
            .items_center()
            .rounded_md()
            .bg(rgb(INSET))
            .border_1()
            .border_color(rgb(BORDER_MUTED))
            .overflow_hidden()
            .text_sm()
            .child(self.stepper("num-dec", "\u{2212}", -(self.step as i64), cx))
            .child(rule())
            .child(div().w(px(40.0)).px_1().child(self.input.clone()))
            .child(rule())
            .child(self.stepper("num-inc", "+", self.step as i64, cx))
    }
}
