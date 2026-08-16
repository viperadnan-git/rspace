//! The tab strip atop the pane, modeled on Zed's `TabBar`/`Tab`.

use super::*;

const TAB_MIN_W: f32 = 90.0;
const TAB_MAX_W: f32 = 200.0;

impl Workspace {
    pub(crate) fn render_tab_strip(&self, g: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let group = &self.groups[g];
        let active = group.active();
        // Zed-style: the right-hand controls (split / preview) live only on the last strip.
        let is_last = g + 1 == self.groups.len();
        let mut tab_els = Vec::with_capacity(group.tabs.len());
        for (ix, tab) in group.tabs.iter().enumerate() {
            tab_els.push(self.render_tab(g, ix, tab, ix == active, cx));
        }
        let tabs = h_flex()
            .id(SharedString::from(format!("tabs-{g}")))
            .h_full()
            .overflow_x_scroll()
            .track_scroll(&group.tab_scroll)
            .children(tab_els)
            // New-tab button trails the last tab and scrolls with them (Chrome-style).
            .child(
                icon_button(SharedString::from(format!("new-tab-{g}")), "icons/plus.svg")
                    .flex_none()
                    .ml_1()
                    .tooltip(tooltip_text("New tab"))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.new_tab_in_group(g, window, cx)
                    })),
            );
        h_flex()
            .id(SharedString::from(format!("tab-bar-{g}")))
            .w_full()
            .flex_none()
            .h(px(PANE_HEADER_H))
            .bg(rgb(INSET))
            .text_sm()
            // Baseline border behind the tabs: inactive tabs show it; the active
            // tab's opaque fill paints over it (connected-tab look).
            .child(
                div()
                    .relative()
                    .flex_1()
                    .h_full()
                    .overflow_x_hidden()
                    // Dropping a tab on the strip's empty area moves it into this group.
                    .drag_over::<DraggedTab>(|s, _, _, _| s.bg(rgba(SELECT_MUTED)))
                    .on_drop(cx.listener(move |this, d: &DraggedTab, window, cx| {
                        this.drop_tab_in_group(d.id, g, window, cx)
                    }))
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .w_full()
                            .h(px(1.0))
                            .bg(rgb(BORDER_MUTED)),
                    )
                    .child(tabs),
            )
            .when(is_last, |bar| {
                bar.child(
                    h_flex()
                        .flex_none()
                        .h_full()
                        .items_center()
                        .border_b_1()
                        .border_color(rgb(BORDER_MUTED))
                        .child(v_divider())
                        .child(
                            h_flex()
                                .px_1()
                                .gap_1()
                                .items_center()
                                .child(
                                    icon_button("toggle-split", "icons/split.svg")
                                        .when(self.is_split(), |b| b.bg(rgba(SELECT_MUTED)))
                                        .tooltip(tooltip_text("Split editor"))
                                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                            this.toggle_split(&ToggleSplit, window, cx)
                                        })),
                                )
                                .child(
                                    icon_button("toggle-preview", "icons/sidebar_right.svg")
                                        .when(self.dock_is(Panel::Preview), |b| b.bg(rgba(SELECT_MUTED)))
                                        .tooltip(tooltip_text("Preview (Space)"))
                                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                            this.toggle_preview(&TogglePreview, window, cx)
                                        })),
                                ),
                        ),
                )
            })
    }

    fn render_tab(&self, _g: usize, _ix: usize, tab: &Tab, active: bool, cx: &mut Context<Self>) -> AnyElement {
        let title = self.tab_title(tab, cx);
        let id = tab.id;
        let pinned = tab.pinned;
        let icon = tab.pane.read(cx).open_remote.as_deref().map(|name| {
            let kind =
                self.remotes.iter().find(|r| r.name == name).map(|r| r.kind.as_str()).unwrap_or("");
            remote_icon(kind)
        });
        let icon = icon.or(pinned.then_some("icons/pin.svg"));
        h_flex()
            .id(SharedString::from(format!("tab-{id}")))
            .flex_none()
            .h_full()
            // Pinned tabs are compact (no close button).
            .min_w(px(if pinned { TAB_MIN_W * 0.6 } else { TAB_MIN_W }))
            .max_w(px(TAB_MAX_W))
            .px_2()
            .gap_1p5()
            .items_center()
            .border_r_1()
            .border_color(rgb(BORDER_MUTED))
            .cursor_pointer()
            .map(|el| {
                if active {
                    el.bg(rgb(CANVAS)).text_color(rgb(FG))
                } else {
                    el.text_color(rgb(FG_MUTED)).hover(|s| s.bg(rgba(OVERLAY)))
                }
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.select_tab_id(id, window, cx)
            }))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                    if !this.is_tab_pinned(id) {
                        this.close_tab_id(id, window, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    this.close_menus();
                    let spec = this.tab_menu_spec(id);
                    this.open_menu(spec, e.position, cx);
                    cx.notify();
                }),
            )
            // Spring-loaded: dragging an entry over the tab activates it after a
            // dwell. `on_drag_move` fires on *every* tab, so gate on whether the
            // cursor is actually inside this tab — otherwise the springs thrash.
            .on_drag_move(cx.listener(move |this, e: &DragMoveEvent<DraggedEntry>, window, cx| {
                if e.bounds.contains(&e.event.position) {
                    this.spring_hover(id, window, cx);
                } else if this.spring.is_pending(&id) {
                    this.spring_clear();
                }
            }))
            .drag_over::<DraggedEntry>(|s, _, _, _| s.bg(rgba(SELECT_MUTED)))
            .on_drop(cx.listener(move |this, d: &DraggedEntry, window, cx| {
                this.drop_into_tab(id, d, window.modifiers(), cx)
            }))
            .on_drag(DraggedTab { id, title: SharedString::from(title.clone()) }, |d, offset, _, cx| {
                let title = d.title.clone();
                cx.new(move |_| DragLabel::new(title, offset))
            })
            .drag_over::<DraggedTab>(|s, _, _, _| s.bg(rgba(SELECT)))
            .on_drop(cx.listener(move |this, d: &DraggedTab, window, cx| this.drop_tab_on(d.id, id, window, cx)))
            .when_some(icon, |el, icon| {
                el.child(svg().path(icon).size(rem(13.0)).flex_shrink_0().text_color(rgb(FG_MUTED)))
            })
            .child(div().flex_1().min_w(px(0.0)).truncate().child(title))
            // Pinned tabs have no close button (closed only from the menu).
            .when(!pinned, |row| {
                row.child(
                    div()
                        .id(SharedString::from(format!("tab-close-{id}")))
                        .flex_shrink_0()
                        .size(rem(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .hover(|s| s.bg(rgba(OVERLAY)))
                        // Swallow the press so closing doesn't also select the tab.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|_, _: &MouseDownEvent, _, cx| cx.stop_propagation()),
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.close_tab_id(id, window, cx)
                        }))
                        .child(svg().path("icons/x.svg").size(rem(12.0)).text_color(rgb(FG_MUTED))),
                )
            })
            .into_any_element()
    }
}
