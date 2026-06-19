//! The single active modal overlay (Zed's `ModalLayer`): one slot holds any
//! modal as an `AnyView` plus its event subscription, replacing a field pair
//! per modal kind. Action side-effects stay in the opener's subscription.

use gpui::AnyView;

use super::*;

/// The modal currently shown over the workspace. At most one is open at a time.
pub(crate) struct ActiveModal {
    view: AnyView,
    /// Anchor near the top (pickers) rather than centered (dialogs).
    align_top: bool,
    /// Render in a deferred layer (so it paints above sibling content).
    deferred: bool,
    /// Run on dismiss, before the view drops (e.g. stop an in-flight OAuth server).
    on_dismiss: Option<Box<dyn Fn(&mut Workspace, &mut Context<Workspace>)>>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl ActiveModal {
    pub(crate) fn new(view: impl Into<AnyView>) -> Self {
        Self {
            view: view.into(),
            align_top: false,
            deferred: false,
            on_dismiss: None,
            _subscriptions: Vec::new(),
        }
    }

    pub(crate) fn align_top(mut self) -> Self {
        self.align_top = true;
        self
    }

    pub(crate) fn deferred(mut self) -> Self {
        self.deferred = true;
        self
    }

    pub(crate) fn on_dismiss(
        mut self,
        f: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Box::new(f));
        self
    }

    pub(crate) fn subscribe(mut self, subscription: gpui::Subscription) -> Self {
        self._subscriptions.push(subscription);
        self
    }
}

impl Workspace {
    pub(crate) fn show_modal(&mut self, modal: ActiveModal, cx: &mut Context<Self>) {
        self.modal = Some(modal);
        cx.notify();
    }

    pub(crate) fn close_modal(&mut self, cx: &mut Context<Self>) {
        if let Some(modal) = self.modal.take() {
            if let Some(on_dismiss) = modal.on_dismiss {
                on_dismiss(self, cx);
            }
            cx.notify();
        }
    }

    /// Whether the open modal is of type `V` (e.g. to toggle the palette shut).
    pub(crate) fn modal_is<V: 'static>(&self) -> bool {
        self.modal.as_ref().is_some_and(|m| m.view.clone().downcast::<V>().is_ok())
    }

    pub(crate) fn render_modal(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let modal = self.modal.as_ref()?;
        Some(self.modal_overlay(
            modal.deferred,
            modal.align_top,
            |this, cx| this.close_modal(cx),
            modal.view.clone(),
            cx,
        ))
    }
}
