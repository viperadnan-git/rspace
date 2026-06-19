//! `Render` for the text field (box chrome, caret line, clear button).

use super::*;

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let border = if self.error.is_some() && !self.bare {
            DANGER
        } else if focused {
            ACCENT
        } else {
            BORDER_MUTED
        };
        let show_clear = self.clearable && !self.bare && focused && !self.content.is_empty();
        let input_box = div()
            .track_focus(&self.focus_handle)
            .key_context("TextInput")
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_to_home))
            .on_action(cx.listener(Self::select_to_end))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_back))
            .on_action(cx.listener(Self::delete_word_forward))
            .on_action(cx.listener(Self::delete_to_start))
            .on_action(cx.listener(Self::delete_to_end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, w, cx| this.on_mouse_down(ev, w, cx)))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, w, cx| this.on_mouse_move(ev, w, cx)))
            // A tab stop, so forms get keyboard navigation automatically.
            .tab_index(0)
            .tab_stop(true)
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .w_full()
            .text_sm()
            .line_height(px(18.0))
            .cursor_text()
            // Box chrome, unless embedded inline (e.g. a list row supplies its own).
            .when(!self.bare, |el| {
                el.h(px(30.0))
                    .px_2()
                    .rounded_md()
                    .bg(rgb(ELEVATED))
                    .border_1()
                    .border_color(rgb(border))
            })
            // Clip the single line so long text scrolls within the field rather
            // than overflowing it; the clear button sits outside the clip.
            .child(div().flex_1().min_w(px(0.0)).overflow_hidden().child(TextElement { input: cx.entity() }))
            .when(show_clear, |el| {
                el.child(
                    div()
                        .id("ti-clear")
                        .flex_none()
                        .size(px(18.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgba(OVERLAY)))
                        // Don't let the clear click also reposition the caret.
                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(|_, _: &MouseDownEvent, _, cx| cx.stop_propagation()))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.clear(window, cx)))
                        .child(svg().path("icons/x.svg").size(px(11.0)).text_color(rgb(FG_MUTED))),
                )
            });
        let error = self.error.clone().filter(|_| !self.bare);
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .w_full()
            .child(input_box)
            .when_some(error, |el, msg| {
                el.child(div().text_xs().text_color(rgb(DANGER)).child(msg))
            })
    }
}
