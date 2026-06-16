//! Transient corner notifications for errors that have no inline home — e.g. a
//! remote delete or initial remote-list load that fails in the background.

use std::time::Duration;

use gpui::ClickEvent;

use super::*;

/// How long a toast stays before auto-dismissing.
const TOAST_TTL: Duration = Duration::from_secs(6);

pub(crate) struct Toast {
    id: usize,
    message: SharedString,
    danger: bool,
}

impl Workspace {
    /// Show a transient notification; it auto-dismisses after [`TOAST_TTL`].
    pub(crate) fn toast(&mut self, message: impl Into<SharedString>, danger: bool, cx: &mut Context<Self>) {
        let id = self.toast_seq;
        self.toast_seq += 1;
        self.toasts.push(Toast { id, message: message.into(), danger });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_TTL).await;
            this.update(cx, |this, cx| this.dismiss_toast(id, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn dismiss_toast(&mut self, id: usize, cx: &mut Context<Self>) {
        self.toasts.retain(|t| t.id != id);
        cx.notify();
    }

    pub(crate) fn render_toasts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div().absolute().bottom_4().right_4().child(
            v_flex().gap_2().items_end().children(self.toasts.iter().map(|t| {
                let id = t.id;
                h_flex()
                    .id(("toast", id))
                    .max_w(px(360.0))
                    .px_3()
                    .py_2()
                    .gap_2()
                    .items_start()
                    .rounded_lg()
                    .bg(rgb(ELEVATED))
                    .border_1()
                    .border_color(rgb(if t.danger { DANGER } else { BORDER_MUTED }))
                    .shadow_lg()
                    .text_sm()
                    .text_color(rgb(FG))
                    .child(div().flex_1().min_w(px(0.0)).child(t.message.clone()))
                    .child(
                        icon_button(("toast-x", id), "icons/x.svg")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.dismiss_toast(id, cx))),
                    )
            })),
        )
    }
}
