//! The explorer column: one [`PaneGroup`] (its tab strip + active tab body), or
//! two side by side when split, divided by a draggable handle.

use super::*;

impl Workspace {
    pub(crate) fn render_explorer(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.groups.len() == 1 {
            return self.render_group_column(0, cx);
        }
        h_flex()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .child(v_flex().w(relative(self.split_ratio)).min_w(px(0.0)).h_full().child(self.render_group_column(0, cx)))
            .child(self.pane_divider(cx))
            .child(v_flex().flex_1().min_w(px(0.0)).h_full().child(self.render_group_column(1, cx)))
            .into_any_element()
    }
}
