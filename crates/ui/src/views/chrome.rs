//! Window chrome: title bar, pane resize handles, modal overlay.

use super::*;

impl Workspace {
    pub(crate) fn resize_handle(
        &self,
        id: &'static str,
        target: ResizeTarget,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let edge = px(-RESIZE_HANDLE_W / 2.0);
        let left_edge = matches!(target, ResizeTarget::Dock);
        deferred(
            div()
                .id(id)
                .absolute()
                .top(px(0.0))
                .when(left_edge, |d| d.left(edge))
                .when(!left_edge, |d| d.right(edge))
                .w(px(RESIZE_HANDLE_W))
                .h_full()
                .cursor_col_resize()
                .occlude()
                .on_drag(DragResize(target), move |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| DragResize(target))
                })
                .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                    if e.click_count() >= 2 {
                        match target {
                            ResizeTarget::Sidebar => this.sidebar.update(cx, |s, cx| s.reset_width(cx)),
                            ResizeTarget::Dock => this.reset_dock_width(cx),
                            // The split divider has its own reset (see `pane_divider`).
                            ResizeTarget::PaneSplit => {}
                        }
                    }
                })),
        )
    }

    fn render_brand(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("brand-home")
            .gap_1p5()
            .px_1()
            .rounded_md()
            .cursor_pointer()
            .hover(|s| s.bg(rgba(OVERLAY)))
            .tooltip(tooltip_text("Home"))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.go_home(window, cx)))
            .child(svg().path("logo.svg").size(rem(15.0)).text_color(rgb(FG)))
            .child(div().text_sm().font_weight(gpui::FontWeight::BOLD).text_color(rgb(FG)).child("rspace"))
    }

    pub(crate) fn render_title_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lead = if window.is_fullscreen() { 12.0 } else { TITLE_BAR_LEAD };
        h_flex()
            // Floor matches the macOS traffic-light strip; grows with content when zoomed.
            .min_h(px(TITLE_BAR_H))
            .py_1()
            .flex_shrink_0()
            .w_full()
            .pl(px(lead))
            .pr_2()
            .justify_between()
            .bg(rgb(INSET))
            .border_b_1()
            .border_color(rgb(BORDER_MUTED))
            .child(self.render_brand(cx))
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        icon_button("keybindings-button", "icons/keyboard.svg")
                            .tooltip(tooltip_text("Keyboard shortcuts"))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_keybindings(cx))),
                    )
                    .child(
                        icon_button("settings-button", "icons/settings.svg")
                            .tooltip(tooltip_text("Settings"))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_settings(cx))),
                    ),
            )
    }


    /// Dim backdrop holding a centered card; clicking outside dismisses.
    pub(crate) fn modal_overlay(
        &self,
        deferred_layer: bool,
        align_top: bool,
        dismiss: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        card: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        // Backdrop: occludes (blocks mouse to the content behind) and dismisses on
        // an outside click. The card is a direct child so its sizing resolves
        // against the full-size overlay; it guards inside-clicks via `modal_surface`.
        let overlay = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .flex()
            .justify_center()
            // Pickers anchor near the top (Zed-style); dialogs center vertically.
            .map(|el| if align_top { el.items_start().pt(px(80.0)) } else { el.items_center() })
            .bg(rgba(0x0000_0099))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| dismiss(this, cx)),
            )
            .child(card);
        if deferred_layer {
            deferred(overlay).priority(3).into_any_element()
        } else {
            overlay.into_any_element()
        }
    }

}
