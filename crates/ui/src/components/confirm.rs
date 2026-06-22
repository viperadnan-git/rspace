//! A confirmation modal entity that emits a choice; the accept action lives in
//! the opener's subscription, keeping the modal presentational.

use gpui::{EventEmitter, FocusHandle, Focusable};

use super::*;

actions!(confirm, [ConfirmAccept]);

pub(crate) enum ConfirmEvent {
    Accepted,
    Dismissed,
}

pub(crate) struct ConfirmModal {
    focus_handle: FocusHandle,
    title: SharedString,
    message: SharedString,
    confirm_label: SharedString,
    danger: bool,
    focused: bool,
}

impl EventEmitter<ConfirmEvent> for ConfirmModal {}

impl Focusable for ConfirmModal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ConfirmModal {
    pub(crate) fn new(
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        confirm_label: impl Into<SharedString>,
        danger: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
            danger,
            focused: false,
        }
    }

    fn accept(&mut self, _: &ConfirmAccept, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(ConfirmEvent::Accepted);
    }
}

impl Render for ConfirmModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        focus_once(&mut self.focused, &self.focus_handle, window, cx);
        let accept_style = if self.danger { ButtonStyle::Danger } else { ButtonStyle::Primary };
        modal_card("confirm-card", &self.focus_handle, cx)
            .key_context("modal Confirm")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::accept))
            .w(rem(400.0))
            .gap_4()
            .child(div().text_lg().text_color(rgb(FG)).child(self.title.clone()))
            .child(div().text_sm().text_color(rgb(FG_MUTED)).child(self.message.clone()))
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("confirm-cancel", "Cancel", ButtonStyle::Secondary).on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ConfirmEvent::Dismissed))))
                    .child(Button::new("confirm-accept", self.confirm_label.clone(), accept_style).on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ConfirmEvent::Accepted)))),
            )
    }
}
