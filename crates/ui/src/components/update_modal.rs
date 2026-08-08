//! Update-available dialog (Skip / Later / Install). The opener's subscription
//! runs the choice, like [`super::confirm`].

use gpui::{EventEmitter, FocusHandle, Focusable};

use super::*;

pub(crate) enum UpdateChoice {
    Install,
    Later,
    Skip,
}

pub(crate) struct UpdateModal {
    focus_handle: FocusHandle,
    version: SharedString,
    notes: Option<SharedString>,
    focused: bool,
}

impl EventEmitter<UpdateChoice> for UpdateModal {}

impl Focusable for UpdateModal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl UpdateModal {
    pub(crate) fn new(
        version: impl Into<SharedString>,
        notes: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self { focus_handle: cx.focus_handle(), version: version.into(), notes, focused: false }
    }
}

impl Render for UpdateModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        focus_once(&mut self.focused, &self.focus_handle, window, cx);
        modal_card("update-card", &self.focus_handle, cx)
            .key_context("modal Update")
            .track_focus(&self.focus_handle)
            .w(rem(420.0))
            .gap_4()
            .child(div().text_lg().text_color(rgb(FG)).child("Update available"))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(FG_MUTED))
                    .child(format!("rspace {} is ready to install.", self.version)),
            )
            .when_some(self.notes.clone(), |el, notes| {
                el.child(
                    div()
                        .id("update-notes")
                        .max_h(rem(220.0))
                        .overflow_y_scroll()
                        .text_xs()
                        .text_color(rgb(FG_MUTED))
                        .child(notes),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .gap_2()
                    .child(
                        Button::new("update-skip", "Skip This Version", ButtonStyle::Secondary)
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(UpdateChoice::Skip))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("update-later", "Later", ButtonStyle::Secondary)
                                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(UpdateChoice::Later))),
                            )
                            .child(
                                Button::new("update-install", "Install", ButtonStyle::Primary)
                                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(UpdateChoice::Install))),
                            ),
                    ),
            )
    }
}
