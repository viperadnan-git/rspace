//! Transient corner notifications. Three shapes share one stack: plain
//! messages (errors/confirmations), a `Pending` spinner that stays until
//! resolved (the promise-toast lifecycle for async ops), and a rich `Info`
//! card (icon + title + labelled rows) for read-only info-op results.
//!
//! Owns its own stack and dismiss timers as a self-contained layer (Zed's
//! `ToastLayer`); the workspace holds it as `Entity<Toasts>` and delegates.

use std::time::Duration;

use gpui::{relative, ClickEvent, FontWeight};

use super::*;

/// How long a resolved/plain toast stays before auto-dismissing.
const TOAST_TTL: Duration = Duration::from_secs(6);
/// Info results linger longer — there's more to read.
const INFO_TTL: Duration = Duration::from_secs(10);

struct Toast {
    id: usize,
    body: ToastBody,
}

/// A toast's content. `Pending` has no TTL until [`Toasts::resolve`] swaps in a
/// result.
pub(crate) enum ToastBody {
    Message { message: SharedString, danger: bool },
    Pending { label: SharedString },
    /// General info result laid out in three tiers: a `title` label (optionally
    /// icon-prefixed) → a `value` subject (e.g. the full `remote:path`) → a quiet
    /// `detail` line of metrics. Each info op composes its own.
    Info {
        icon: Option<&'static str>,
        title: SharedString,
        value: Option<SharedString>,
        detail: Option<SharedString>,
    },
}

pub(crate) struct Toasts {
    items: Vec<Toast>,
    seq: usize,
    /// Dismiss timers pause while the window is unfocused (Sonner-style), so a
    /// toast can't expire while the user isn't looking.
    window_active: bool,
}

impl Toasts {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_window_activation(window, |this, window, _| {
            this.window_active = window.is_window_active();
        })
        .detach();
        Self { items: Vec::new(), seq: 0, window_active: true }
    }

    /// Show a transient message; auto-dismisses after [`TOAST_TTL`].
    pub(crate) fn toast(&mut self, message: impl Into<SharedString>, danger: bool, cx: &mut Context<Self>) {
        self.push(ToastBody::Message { message: message.into(), danger }, Some(TOAST_TTL), cx);
    }

    /// Show a message that stays until the user dismisses it (no auto-dismiss).
    pub(crate) fn toast_sticky(&mut self, message: impl Into<SharedString>, danger: bool, cx: &mut Context<Self>) {
        self.push(ToastBody::Message { message: message.into(), danger }, None, cx);
    }

    /// Show a spinner toast that stays until [`Self::resolve`]; returns its id so
    /// the caller can resolve it when its async work completes.
    pub(crate) fn toast_pending(&mut self, label: impl Into<SharedString>, cx: &mut Context<Self>) -> usize {
        self.push(ToastBody::Pending { label: label.into() }, None, cx)
    }

    /// Replace a pending toast's content with its result. When `auto_dismiss`,
    /// it fades after a TTL; otherwise it stays until the user dismisses it.
    /// Falls back to a fresh toast if the pending one was dismissed.
    pub(crate) fn resolve(&mut self, id: usize, body: ToastBody, auto_dismiss: bool, cx: &mut Context<Self>) {
        let ttl = auto_dismiss
            .then(|| if matches!(body, ToastBody::Info { .. }) { INFO_TTL } else { TOAST_TTL });
        match self.items.iter_mut().find(|t| t.id == id) {
            Some(t) => t.body = body,
            None => {
                self.push(body, ttl, cx);
                return;
            }
        }
        if let Some(ttl) = ttl {
            self.schedule_dismiss(id, ttl, cx);
        }
        cx.notify();
    }

    fn push(&mut self, body: ToastBody, ttl: Option<Duration>, cx: &mut Context<Self>) -> usize {
        let id = self.seq;
        self.seq += 1;
        self.items.push(Toast { id, body });
        if let Some(ttl) = ttl {
            self.schedule_dismiss(id, ttl, cx);
        }
        cx.notify();
        id
    }

    fn schedule_dismiss(&self, id: usize, ttl: Duration, cx: &mut Context<Self>) {
        // Poll in small ticks and accumulate active time until it reaches the TTL.
        cx.spawn(async move |this, cx| {
            let tick = Duration::from_millis(200);
            let mut active_elapsed = Duration::ZERO;
            while active_elapsed < ttl {
                cx.background_executor().timer(tick).await;
                let Ok((present, active)) = this.update(cx, |this, _| {
                    (this.items.iter().any(|t| t.id == id), this.window_active)
                }) else {
                    return;
                };
                if !present {
                    return; // dismissed manually or replaced
                }
                if active {
                    active_elapsed += tick;
                }
            }
            this.update(cx, |this, cx| this.dismiss(id, cx)).ok();
        })
        .detach();
    }

    fn dismiss(&mut self, id: usize, cx: &mut Context<Self>) {
        self.items.retain(|t| t.id != id);
        cx.notify();
    }

    fn card(&self, toast: &Toast, cx: &mut Context<Self>) -> gpui::AnyElement {
        let id = toast.id;
        let card = h_flex()
            .id(("toast", id))
            .max_w(px(360.0))
            .px_3()
            .py_2()
            .gap_2()
            .items_center()
            .rounded_lg()
            .bg(rgb(ELEVATED))
            .border_1()
            .border_color(rgb(BORDER_MUTED))
            .shadow_lg()
            .text_sm()
            .text_color(rgb(FG));
        let dismiss = icon_button(("toast-x", id), "icons/x.svg")
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.dismiss(id, cx)));
        match &toast.body {
            ToastBody::Pending { label } => card
                .child(spinner(("toast-spin", id), px(14.0), FG_MUTED))
                .child(div().flex_1().min_w(px(0.0)).line_height(relative(1.)).text_color(rgb(FG_MUTED)).child(label.clone()))
                .into_any_element(),
            ToastBody::Message { message, danger } => card
                .child(
                    svg()
                        .path(if *danger { "icons/alert.svg" } else { "icons/check.svg" })
                        .size(px(15.0))
                        .flex_shrink_0()
                        .text_color(rgb(if *danger { DANGER } else { SUCCESS })),
                )
                .child(div().flex_1().min_w(px(0.0)).line_height(relative(1.2)).child(message.clone()))
                .child(dismiss)
                .into_any_element(),
            ToastBody::Info { icon, title, value, detail } => card
                .items_start()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap_1()
                        // Label row — what kind of result this is (optional leading icon).
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                // Cap-center nudge: the title line box centers on its em center,
                                // above the cap center (ascent padding; gpui has no leading-trim).
                                .children((*icon).map(|i| {
                                    svg().path(i).size(px(15.0)).flex_shrink_0().mt(px(1.0)).text_color(rgb(FG_MUTED))
                                }))
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .line_height(relative(1.))
                                        .child(title.clone()),
                                ),
                        )
                        // Subject — the full path (wraps; the identifying line).
                        .children(value.clone().map(|v| {
                            div().line_height(relative(1.3)).text_color(rgb(FG)).child(v)
                        }))
                        // Metrics — quiet supporting numbers.
                        .children(detail.clone().map(|d| {
                            div().text_xs().text_color(rgb(FG_MUTED)).child(d)
                        })),
                )
                .child(dismiss)
                .into_any_element(),
        }
    }
}

impl Render for Toasts {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Deferred above the modal overlays (priority 3) so toasts surface on top
        // of Settings/confirm dialogs rather than behind them.
        deferred(
            div().absolute().bottom_4().right_4().child(
                v_flex().gap_2().items_end().children(self.items.iter().map(|t| self.card(t, cx))),
            ),
        )
        .priority(4)
    }
}
