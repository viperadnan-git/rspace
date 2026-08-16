//! Thin delegates onto the [`Toasts`] layer entity.

use super::*;

impl Workspace {
    pub(crate) fn toast(&mut self, message: impl Into<SharedString>, danger: bool, cx: &mut Context<Self>) {
        self.toasts.update(cx, |t, cx| t.toast(message, danger, cx));
    }

    pub(crate) fn toast_sticky(&mut self, message: impl Into<SharedString>, danger: bool, cx: &mut Context<Self>) {
        self.toasts.update(cx, |t, cx| t.toast_sticky(message, danger, cx));
    }

    pub(crate) fn toast_pending(&mut self, label: impl Into<SharedString>, cx: &mut Context<Self>) -> usize {
        self.toasts.update(cx, |t, cx| t.toast_pending(label, cx))
    }

    pub(crate) fn resolve_toast(&mut self, id: usize, body: ToastBody, auto_dismiss: bool, cx: &mut Context<Self>) {
        self.toasts.update(cx, |t, cx| t.resolve(id, body, auto_dismiss, cx));
    }

    /// Post an OS notification for a finished transfer — but only when the window
    /// isn't frontmost; an in-app toast already covers the case where the user is
    /// looking. One shared tag, so a newer finish replaces the last (no stacking).
    pub(crate) fn notify_transfer(
        &self,
        label: &SharedString,
        ok: bool,
        error: Option<&SharedString>,
        cx: &mut Context<Self>,
    ) {
        if self.window_active {
            return;
        }
        let (title, body) = if ok {
            ("Transfer complete".into(), label.clone())
        } else {
            let body = match error {
                Some(e) => format!("{label} \u{2014} {e}").into(),
                None => label.clone(),
            };
            ("Transfer failed".into(), body)
        };
        cx.show_system_notification(gpui::SystemNotification {
            tag: "rspace-transfer".into(),
            title,
            body,
            actions: Vec::new(),
        });
    }
}
