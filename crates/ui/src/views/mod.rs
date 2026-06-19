//! Workspace views: title bar, sidebar, file explorer, welcome screen.

use super::*;

mod chrome;
mod explorer;
mod sidebar;
mod welcome;

impl Workspace {
    fn entry_drop_target(
        &self,
        el: Stateful<Div>,
        dst_remote: String,
        dst_dir: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        el.drag_over::<DraggedEntry>(|s, _, _, _| s.bg(rgba(SELECT))).on_drop(cx.listener(
            move |this, d: &DraggedEntry, window, cx| {
                this.drop_into(d, dst_remote.clone(), dst_dir.clone(), window.modifiers().alt, cx)
            },
        ))
    }

}
