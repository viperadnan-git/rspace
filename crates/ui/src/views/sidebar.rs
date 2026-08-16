//! Sidebar chrome: the fixed-width, resizable column that hosts the [`Sidebar`]
//! pane entity. The remote list itself lives in the sidebar view.

use super::*;

impl Workspace {
    pub(crate) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .relative()
            .w(self.sidebar.read(cx).width())
            .flex_shrink_0()
            .overflow_hidden()
            .bg(rgb(INSET))
            .border_r_1()
            .border_color(rgb(BORDER_MUTED))
            .child(self.resize_handle("sidebar-resize", ResizeTarget::Sidebar, cx))
            .child(self.sidebar.clone())
    }
}
