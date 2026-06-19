//! `Render` for the workspace (the whole two-pane layout).

use super::*;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.rebuild_search_view();
        self.resolve_selection();
        self.refresh_preview(cx);
        // Keep focus on the open dialog, else on the workspace — so each owns the
        // keyboard while shown, and focus returns here when it closes. The modal
        // entities (remote config, confirm) steer their own focus.
        if self.modal.is_some() || self.prompt.is_some() {
        } else if self.settings_open {
            // Settings inputs own their focus; focus a freshly-opened rclone edit
            // input once, then leave it be (re-focusing each frame would trap it).
            if let Some((_, input)) = self.rclone_edit.clone() {
                let handle = input.read(cx).focus_handle(cx);
                focus_once(&mut self.rclone_edit_focus, &handle, window, cx);
            }
        } else if self.search_input.read(cx).focus_handle(cx).is_focused(window) {
        } else if !self.focus.is_focused(window) {
            self.focus.focus(window, cx);
        }
        v_flex()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::go_up))
            .on_action(cx.listener(Self::action_back))
            .on_action(cx.listener(Self::action_forward))
            .on_action(cx.listener(Self::reload))
            .on_action(cx.listener(Self::minimize))
            .on_action(cx.listener(Self::zoom))
            .on_action(cx.listener(Self::toggle_fullscreen))
            .on_action(cx.listener(Self::close_settings))
            .on_action(cx.listener(Self::toggle_pane))
            .on_action(cx.listener(Self::toggle_search_action))
            .on_action(cx.listener(Self::focus_sidebar))
            .on_action(cx.listener(Self::focus_explorer))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::new_folder))
            .on_action(cx.listener(Self::new_file))
            .on_action(cx.listener(Self::rename))
            .on_action(cx.listener(Self::toggle_preview))
            .on_action(cx.listener(Self::toggle_palette))
            .on_action(cx.listener(Self::action_add_remote))
            .on_action(cx.listener(Self::action_open_settings))
            .on_action(cx.listener(Self::action_restart_daemon))
            .on_action(cx.listener(Self::action_toggle_transfers))
            .on_drag_move(cx.listener(|this, e: &DragMoveEvent<DragResize>, window, cx| {
                let x = f32::from(e.event.position.x);
                let (width, current) = match e.drag(cx).0 {
                    ResizeTarget::Sidebar => {
                        (px(x.clamp(SIDEBAR_MIN, SIDEBAR_MAX)), &mut this.sidebar_width)
                    }
                    ResizeTarget::Preview => {
                        // Pane is docked right: width grows as the cursor nears the edge.
                        let from_right = f32::from(window.viewport_size().width) - x;
                        (px(from_right.clamp(PREVIEW_MIN, PREVIEW_MAX)), &mut this.preview_width)
                    }
                };
                if width != *current {
                    *current = width;
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::persist_pane_widths))
            .size_full()
            .bg(rgb(CANVAS))
            .text_color(rgb(FG))
            .text_sm()
            .child(self.render_title_bar(window, cx))
            .child({
                // A panel covers the browser only while open AND zoomed, so
                // closing it can never leave the content region blank.
                let zoomed = self.jobs_open && self.jobs_maximized;
                v_flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .when(!zoomed, |el| {
                        el.child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_1()
                                .min_h(px(0.0))
                                .w_full()
                                .child(self.render_sidebar(cx))
                                .child(self.render_explorer(cx)),
                        )
                    })
                    .when(self.jobs_open, |el| el.child(self.render_transfers(cx)))
            })
            .child(self.render_status_bar(cx))
            .when(self.context.is_some(), |el| el.child(self.render_context_menu(cx)))
            .when(self.remote_menu.is_some(), |el| el.child(self.render_remote_menu(cx)))
            .when(self.bg_menu.is_some(), |el| el.child(self.render_bg_menu(cx)))
            .when(self.rc_popover_open, |el| el.child(self.rc_popover_backdrop(cx)))
            .when(self.settings_open, |el| el.child(self.render_settings(cx)))
            .children(self.render_modal(cx))
            .child(self.toasts.clone())
    }
}
