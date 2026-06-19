//! `Render` for the workspace (the whole two-pane layout).

use super::*;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Restore focus only when it has been lost (e.g. a modal closed) — route
        // it to the active pane. Modals, the inline prompt, the settings panel,
        // and the explorer (incl. its search input) each own their own focus.
        let explorer_focused = self.explorer.focus_handle(cx).contains_focused(window, cx);
        let sidebar_focused = self.sidebar.focus_handle(cx).contains_focused(window, cx);
        if self.modal.is_some() || self.prompt.is_some() {
        } else if self.settings.open {
            // Settings inputs own their focus; focus a freshly-opened rclone edit
            // input once, then leave it be (re-focusing each frame would trap it).
            if let Some((_, input)) = self.settings.rclone_edit.clone() {
                let handle = input.read(cx).focus_handle(cx);
                focus_once(&mut self.settings.rclone_edit_focus, &handle, window, cx);
            }
        } else if !explorer_focused && !sidebar_focused && !self.focus.is_focused(window) {
            // Focus lost (e.g. a modal closed): route to the active pane.
            if self.open_remote.is_some() {
                self.focus_explorer_pane(window, cx);
            } else {
                self.focus_sidebar_pane(window, cx);
            }
        }
        v_flex()
            .key_context("Workspace")
            .track_focus(&self.focus)
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
                match e.drag(cx).0 {
                    ResizeTarget::Sidebar => {
                        let w = px(x.clamp(SIDEBAR_MIN, SIDEBAR_MAX));
                        this.sidebar.update(cx, |s, cx| s.set_width(w, cx));
                    }
                    ResizeTarget::Preview => {
                        // Pane is docked right: width grows as the cursor nears the edge.
                        let from_right = f32::from(window.viewport_size().width) - x;
                        let w = px(from_right.clamp(PREVIEW_MIN, PREVIEW_MAX));
                        this.preview.update(cx, |p, cx| p.set_width(w, cx));
                    }
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
            .when(self.menus.context.is_some(), |el| el.child(self.render_context_menu(cx)))
            .when(self.menus.remote_menu.is_some(), |el| el.child(self.render_remote_menu(cx)))
            .when(self.menus.bg_menu.is_some(), |el| el.child(self.render_bg_menu(cx)))
            .when(self.menus.rc_popover_open, |el| el.child(self.rc_popover_backdrop(cx)))
            .when(self.settings.open, |el| el.child(self.render_settings(cx)))
            .children(self.render_modal(cx))
            .child(self.toasts.clone())
    }
}
