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
}
