//! Keyboard navigation and focus cycling for the remote dialog (see [`super`]).

use super::*;

impl RemoteConfigModal {
    pub(crate) fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_focus(1, window, cx);
    }

    pub(crate) fn focus_prev(&mut self, _: &FocusPrev, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_focus(-1, window, cx);
    }

    /// Tab / Shift-Tab through every focusable control in the dialog, in visual
    /// order. `window.focus_next` traverses the whole window (background panes
    /// included), so the modal drives its own contained cycle.
    fn cycle_focus(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let mut handles: Vec<gpui::FocusHandle> = Vec::new();
        match self.phase {
            Phase::Form => {
                handles.push(self.name.read(cx).focus_handle(cx));
                let push_opt = |opt: &RemoteOption, handles: &mut Vec<gpui::FocusHandle>| {
                    if opt.kind == "bool" {
                        if let Some(h) = self.bool_focus.get(&opt.name) {
                            handles.push(h.clone());
                        }
                    } else if let Some(input) = self.fields.get(&opt.name) {
                        handles.push(input.read(cx).focus_handle(cx));
                    }
                };
                // Basic fields, then advanced (matching the form's visual order).
                for opt in self.options.iter().filter(|o| !o.advanced) {
                    push_opt(opt, &mut handles);
                }
                if self.options.iter().any(|o| o.advanced) {
                    handles.push(self.advanced_focus.clone());
                    if self.show_advanced {
                        for opt in self.options.iter().filter(|o| o.advanced) {
                            push_opt(opt, &mut handles);
                        }
                    }
                }
                handles.push(self.cancel_focus.clone());
                handles.push(self.primary_focus.clone());
                handles.push(self.close_focus.clone());
            }
            Phase::Question => {
                handles.push(self.answer.read(cx).focus_handle(cx));
                handles.push(self.primary_focus.clone());
                handles.push(self.close_focus.clone());
            }
            Phase::PickType | Phase::Busy => return,
        }
        if handles.is_empty() {
            return;
        }
        let current = handles.iter().position(|h| h.is_focused(window));
        let next = match current {
            Some(i) => (i as i32 + delta).rem_euclid(handles.len() as i32) as usize,
            None => 0,
        };
        handles[next].focus(window, cx);
        cx.notify();
    }

    pub(crate) fn config_next(&mut self, _: &ConfigNext, _: &mut Window, cx: &mut Context<Self>) {
        self.config_nav(1, cx);
    }

    pub(crate) fn config_prev(&mut self, _: &ConfigPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.config_nav(-1, cx);
    }

    fn config_nav(&mut self, delta: i32, cx: &mut Context<Self>) {
        if self.phase != Phase::PickType {
            return;
        }
        let len = self.filtered_backends(cx).len();
        if len == 0 {
            return;
        }
        let cur = self.picker_sel.min(len - 1) as i32;
        let next = (cur + delta).rem_euclid(len as i32) as usize;
        self.picker_sel = next;
        self.picker_scroll.scroll_to_item(next, ScrollStrategy::Nearest);
        cx.notify();
    }

    pub(crate) fn config_confirm(&mut self, _: &ConfigConfirm, _: &mut Window, cx: &mut Context<Self>) {
        match self.phase {
            Phase::PickType => {
                let names = self.filtered_backends(cx);
                if let Some((name, _)) = names.get(self.picker_sel.min(names.len().saturating_sub(1))) {
                    self.pick_backend(name.clone(), cx);
                }
            }
            Phase::Form => self.submit_config(cx),
            Phase::Question => self.answer_question(cx),
            Phase::Busy => {}
        }
    }
}
