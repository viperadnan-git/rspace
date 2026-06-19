//! The remotes sidebar.

use super::*;

impl Workspace {
    fn remote_row(
        &self,
        ix: usize,
        remote: RemoteInfo,
        pinned: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let focused = self.pane == Pane::Sidebar;
        // No highlight on the landing view (nothing open).
        let selected = self.open_remote.is_some() && ix == self.remote_sel;
        let menu_name = remote.name.clone();
        let mut row = nav_item(ix, selected, focused)
            .tooltip(tooltip_text(format!("{} · {}", remote.name, remote.kind)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.load_remote(ix, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    this.remote_menu = Some((menu_name.clone(), ev.position));
                    cx.notify();
                }),
            )
            .child(
                svg()
                    .path(remote_icon(&remote.kind))
                    .size(px(15.0))
                    .flex_shrink_0()
                    .text_color(rgb(FG_MUTED)),
            )
            .child(
                div()
                    .flex_grow(1.0)
                    .min_w(px(0.0))
                    .truncate()
                    .text_color(rgb(FG))
                    .child(remote.name.clone()),
            )
            .when(self.mounted.contains(&remote.name), |r| {
                r.child(
                    svg()
                        .path("icons/hard_drive.svg")
                        .size(px(11.0))
                        .flex_shrink_0()
                        .text_color(rgb(ACCENT)),
                )
            })
            .when(pinned, |r| {
                r.child(svg().path("icons/pin.svg").size(px(11.0)).flex_shrink_0().text_color(rgb(FG_SUBTLE)))
            });

        row = self.entry_drop_target(row, remote.name.clone(), String::new(), cx);

        if pinned {
            let drag_name = remote.name.clone();
            let target = remote.name.clone();
            row = row
                .on_drag(DraggedRemote { name: drag_name }, |d, _, _, app| {
                    app.new(|_| DragLabel { text: d.name.clone().into() })
                })
                .drag_over::<DraggedRemote>(|s, _, _, _| s.bg(rgba(SELECT_MUTED)))
                .on_drop(cx.listener(move |this, d: &DraggedRemote, _, cx| {
                    this.reorder_pinned(&d.name, &target, cx)
                }));
        }
        row
    }

    pub(crate) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.remotes.len();
        v_flex()
            .relative()
            .w(self.sidebar_width)
            .flex_shrink_0()
            .overflow_hidden()
            .bg(rgb(INSET))
            .border_r_1()
            .border_color(rgb(BORDER_MUTED))
            .child(self.resize_handle("sidebar-resize", ResizeTarget::Sidebar, SIDEBAR_W, cx))
            .child(
                h_flex()
                    .w_full()
                    .px_3()
                    .py_2()
                    .justify_between()
                    .items_center()
                    .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child("REMOTES"))
                    .child(
                        icon_button("add-remote", "icons/plus.svg")
                            .tooltip(tooltip_text("Add remote"))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.begin_add_remote(cx))),
                    ),
            )
            .child(
                // Single list so pinned rows (which lead it) scroll with the rest.
                uniform_list(
                    "remotes",
                    count,
                    cx.processor(|this, range: Range<usize>, _window, cx| {
                        let ordered = this.ordered_remotes();
                        let pinned_count = this.pinned_remotes().len();
                        range
                            .filter_map(|ix| ordered.get(ix).map(|r| (ix, r.clone())))
                            .map(|(ix, remote)| this.remote_row(ix, remote, ix < pinned_count, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.remote_scroll)
                .px_1()
                .flex_1(),
            )
    }

}
