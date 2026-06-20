//! The explorer column: the [`ActionBar`] entity (back/forward · locator ·
//! actions) above the [`Explorer`] pane entity, with the preview beside it. The
//! listing itself lives in the explorer view.

use super::*;

impl Workspace {
    /// The pane: the tab strip plus the active tab's body (welcome screen, or the
    /// file-list column with its preview).
    pub(crate) fn render_explorer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .child(self.render_tab_strip(cx))
            .child(self.render_pane_body(cx))
    }

    fn render_pane_body(&self, cx: &mut Context<Self>) -> AnyElement {
        // The welcome screen replaces the body when no remote is open.
        if self.active().open_remote.is_none() {
            return self.render_welcome(cx).into_any_element();
        }
        v_flex()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .overflow_hidden()
            .bg(rgb(INSET))
            .child(self.action_bar.clone())
            .child(self.explorer())
            .into_any_element()
    }
}
