//! `Render` for the workspace (the whole two-pane layout).

use super::*;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The UI font size is the rem size: it scales all rem-based text + sizing.
        window.set_rem_size(px(self.ui_font_size()));
        // Restore focus only when it has been lost (e.g. a modal closed) — route it
        // to the active pane. Each pane (and the search field, which lives outside
        // the explorer subtree) owns its focus; `focusable_panes` is the registry
        // the guard consults, so a new pane is added there, not as a special case.
        if self.modal.is_some() || self.prompt.is_some() {
        } else if self.settings.open {
            // Settings inputs own their focus; focus a freshly-opened rclone edit
            // input once, then leave it be (re-focusing each frame would trap it).
            if let Some((_, input)) = self.settings.rclone_edit.clone() {
                let handle = input.read(cx).focus_handle(cx);
                focus_once(&mut self.settings.rclone_edit_focus, &handle, window, cx);
            }
        } else if !self.any_pane_focused(window, cx) {
            // Focus lost (e.g. a modal closed): route to the active pane.
            if self.active().open_remote.is_some() {
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
            .on_action(cx.listener(Self::action_show_keybindings))
            .on_action(cx.listener(Self::action_restart_daemon))
            .on_action(cx.listener(Self::action_toggle_tasks))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::prev_tab))
            .on_action(cx.listener(|this, _: &ActivateTab1, w, cx| this.jump_to_tab(1, w, cx)))
            .on_action(cx.listener(|this, _: &ActivateTab2, w, cx| this.jump_to_tab(2, w, cx)))
            .on_action(cx.listener(|this, _: &ActivateTab3, w, cx| this.jump_to_tab(3, w, cx)))
            .on_action(cx.listener(|this, _: &ActivateTab4, w, cx| this.jump_to_tab(4, w, cx)))
            .on_action(cx.listener(|this, _: &ActivateTab5, w, cx| this.jump_to_tab(5, w, cx)))
            .on_action(cx.listener(|this, _: &ActivateTab6, w, cx| this.jump_to_tab(6, w, cx)))
            .on_action(cx.listener(|this, _: &ActivateTab7, w, cx| this.jump_to_tab(7, w, cx)))
            .on_action(cx.listener(|this, _: &ActivateTab8, w, cx| this.jump_to_tab(8, w, cx)))
            .on_action(cx.listener(|this, _: &ActivateTab9, w, cx| this.jump_to_tab(9, w, cx)))
            .on_drag_move(cx.listener(|this, e: &DragMoveEvent<DragResize>, window, cx| {
                let x = f32::from(e.event.position.x);
                match e.drag(cx).0 {
                    ResizeTarget::Sidebar => {
                        let w = px(x.clamp(SIDEBAR_MIN, SIDEBAR_MAX));
                        this.sidebar.update(cx, |s, cx| s.set_width(w, cx));
                    }
                    ResizeTarget::Dock => {
                        // Docked right: width grows as the cursor nears the edge.
                        let from_right = f32::from(window.viewport_size().width) - x;
                        let w = px(from_right.clamp(PREVIEW_MIN, PREVIEW_MAX));
                        this.set_dock_width(w, cx);
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::persist_pane_widths))
            .size_full()
            .bg(rgb(CANVAS))
            .text_color(rgb(FG))
            .text_sm()
            .child(self.render_title_bar(window, cx))
            .child(
                // Three columns: sidebar | the active pane (tab strip + browser) |
                // the right dock (one panel: preview xor tasks).
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.0))
                    .w_full()
                    .child(self.render_sidebar(cx))
                    .child(self.render_explorer(cx))
                    .children(self.render_dock(cx)),
            )
            .child(self.render_status_bar(cx))
            .when(self.menus.context.is_some(), |el| el.child(self.render_context_menu(cx)))
            .when(self.menus.task_menu.is_some(), |el| el.child(self.render_task_menu(cx)))
            .when(self.menus.remote_menu.is_some(), |el| el.child(self.render_remote_menu(cx)))
            .when(self.menus.tab_menu.is_some(), |el| el.child(self.render_tab_menu(cx)))
            .when(self.menus.bg_menu.is_some(), |el| el.child(self.render_bg_menu(cx)))
            .when(self.menus.rc_popover_open, |el| el.child(self.rc_popover_backdrop(cx)))
            .when(self.settings.open, |el| el.child(self.render_settings(cx)))
            .children(self.render_modal(cx))
            .child(self.toasts.clone())
    }
}
