//! Rendering for the [`Sidebar`]: the "REMOTES" header and the remote list.

use super::*;

impl Sidebar {
    /// `flags` = (pinned, mounted, selected, focused) — packed to keep the arg
    /// count in check.
    fn remote_row(
        &self,
        ix: usize,
        remote: RemoteInfo,
        flags: (bool, bool, bool, bool),
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (pinned, mounted, selected, focused) = flags;
        let menu_name = remote.name.clone();
        let mut row = nav_item(ix, selected, focused)
            .tooltip(tooltip_text(format!("{} · {}", remote.name, remote.kind)))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.focus_explorer(window, cx);
                cx.emit(SidebarEvent::Open(ix));
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |_, ev: &MouseDownEvent, _, cx| {
                    cx.emit(SidebarEvent::Menu(menu_name.clone(), ev.position));
                }),
            )
            .child(svg().path(remote_icon(&remote.kind)).size(rem(15.0)).flex_shrink_0().text_color(rgb(FG_MUTED)))
            .child(
                div()
                    .flex_grow(1.0)
                    .min_w(px(0.0))
                    .truncate()
                    .text_color(rgb(FG))
                    .child(remote.name.clone()),
            )
            .when(mounted, |r| {
                r.child(svg().path("icons/hard_drive.svg").size(rem(11.0)).flex_shrink_0().text_color(rgb(ACCENT)))
            })
            .when(pinned, |r| {
                r.child(svg().path("icons/pin.svg").size(rem(11.0)).flex_shrink_0().text_color(rgb(FG_SUBTLE)))
            });

        // Drop an explorer entry onto a remote to move it to that remote's root.
        let dst = remote.name.clone();
        row = row
            .drag_over::<DraggedEntry>(|s, _, _, _| s.bg(rgba(SELECT_MUTED)))
            .on_drop(cx.listener(move |_, dragged: &DraggedEntry, _, cx| {
                cx.emit(SidebarEvent::DropEntry { dragged: dragged.clone(), dst_remote: dst.clone() });
            }));

        if pinned {
            let drag_name = remote.name.clone();
            let target = remote.name.clone();
            row = row
                .on_drag(DraggedRemote { name: drag_name }, |d, _, _, app| {
                    app.new(|_| DragLabel { text: d.name.clone().into() })
                })
                .drag_over::<DraggedRemote>(|s, _, _, _| s.bg(rgba(SELECT_MUTED)))
                .on_drop(cx.listener(move |_, d: &DraggedRemote, _, cx| {
                    cx.emit(SidebarEvent::Reorder { from: d.name.clone(), before: target.clone() });
                }));
        }
        row
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (ordered, pinned_count, open, mounted) = self
            .workspace
            .upgrade()
            .map(|ws| {
                let ws = ws.read(cx);
                (ws.ordered_remotes(), ws.pinned_count(), ws.has_open_remote(), ws.mounted_set())
            })
            .unwrap_or_default();
        let count = ordered.len();
        v_flex()
            .key_context("Sidebar")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::open))
            .size_full()
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
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(SidebarEvent::Add))),
                    ),
            )
            .child(
                // Single list so pinned rows (which lead it) scroll with the rest.
                uniform_list(
                    "remotes",
                    count,
                    cx.processor(move |this, range: Range<usize>, window, cx| {
                        let focused = this.focus.is_focused(window);
                        range
                            .filter_map(|ix| ordered.get(ix).map(|r| (ix, r.clone())))
                            .map(|(ix, remote)| {
                                let selected = open && ix == this.remote_sel;
                                let flags = (ix < pinned_count, mounted.contains(&remote.name), selected, focused);
                                this.remote_row(ix, remote, flags, cx)
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.remote_scroll)
                .px_1()
                .flex_1(),
            )
    }
}
