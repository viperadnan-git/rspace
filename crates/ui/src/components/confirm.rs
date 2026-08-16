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
    /// When set, accept stays disabled until the user types this exact text —
    /// the guard for an irreversible whole-remote action.
    require: Option<SharedString>,
    input: Option<Entity<TextInput>>,
    _input_sub: Option<gpui::Subscription>,
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
            require: None,
            input: None,
            _input_sub: None,
        }
    }

    /// Gate accept behind typing `text` verbatim.
    pub(crate) fn require_text(mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let text = text.into();
        let input = cx.new(|cx| TextInput::new(cx, text.to_string()));
        // Re-render as they type so the accept button unlocks on the match.
        self._input_sub = Some(cx.observe(&input, |_, _, cx| cx.notify()));
        self.input = Some(input);
        self.require = Some(text);
        self
    }

    /// Whether accept is allowed — always, unless a typed confirmation is pending.
    fn can_accept(&self, cx: &App) -> bool {
        match (&self.require, &self.input) {
            (Some(want), Some(input)) => input.read(cx).text().trim() == want.as_ref(),
            _ => true,
        }
    }

    fn accept(&mut self, _: &ConfirmAccept, _: &mut Window, cx: &mut Context<Self>) {
        if self.can_accept(cx) {
            cx.emit(ConfirmEvent::Accepted);
        }
    }
}

impl Render for ConfirmModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        focus_once(&mut self.focused, &self.focus_handle, window, cx);
        let accept_style = if self.danger { ButtonStyle::Danger } else { ButtonStyle::Primary };
        let can_accept = self.can_accept(cx);
        modal_card("confirm-card", &self.focus_handle, cx)
            .key_context("modal Confirm")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::accept))
            .w(rem(400.0))
            .gap_4()
            .child(div().text_lg().text_color(rgb(FG)).child(self.title.clone()))
            .child(div().text_sm().text_color(rgb(FG_MUTED)).child(self.message.clone()))
            .when_some(self.require.clone().zip(self.input.clone()), |el, (want, input)| {
                el.child(
                    v_flex()
                        .gap_1p5()
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(FG_MUTED))
                                .child(format!("Type \u{201c}{want}\u{201d} to confirm:")),
                        )
                        .child(input),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("confirm-cancel", "Cancel", ButtonStyle::Ghost).on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ConfirmEvent::Dismissed))))
                    .child(
                        Button::new("confirm-accept", self.confirm_label.clone(), accept_style)
                            .disabled(!can_accept)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                if this.can_accept(cx) {
                                    cx.emit(ConfirmEvent::Accepted);
                                }
                            })),
                    ),
            )
    }
}
