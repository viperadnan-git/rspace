//! A reusable command-menu / picker: a modal card with a query input and a
//! fuzzy-filtered, keyboard-navigable result list. The behaviour lives here;
//! each use supplies a [`PickerDelegate`] for its items. Modeled on Zed's
//! `Picker` so the command palette, remote picker, etc. share one primitive.

use gpui::{
    actions, uniform_list, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    KeyBinding, ListSizingBehavior, MouseButton, MouseDownEvent, ScrollStrategy, Subscription,
    UniformListScrollHandle, Window,
};

use super::*;
use crate::text_input::{TextInput, TextInputEvent};

actions!(picker, [SelectPrev, SelectNext, Confirm]);

/// Outcome of confirming a match.
pub enum Confirmed {
    /// Close the picker.
    Dismiss,
    /// Stay open and start a fresh stage: clears the query and re-reads the
    /// placeholder/matches (e.g. advanced to the next argument).
    Continue,
    /// Stay in the current stage but replace the query text (e.g. descend into a
    /// folder by completing its path).
    SetQuery(String),
}

/// Bind the picker's navigation keys (call once at startup).
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, Some("Picker")),
        KeyBinding::new("ctrl-p", SelectPrev, Some("Picker")),
        KeyBinding::new("down", SelectNext, Some("Picker")),
        KeyBinding::new("ctrl-n", SelectNext, Some("Picker")),
        KeyBinding::new("enter", Confirm, Some("Picker")),
    ]);
}

/// Supplies a [`Picker`]'s items, filtering, rendering, and confirm behaviour.
pub trait PickerDelegate: Sized + 'static {
    /// Prompt for the current stage (re-read whenever a stage starts).
    fn placeholder(&self) -> SharedString;
    fn match_count(&self) -> usize;
    fn selected_index(&self) -> usize;
    fn set_selected_index(&mut self, ix: usize, cx: &mut Context<Picker<Self>>);
    /// Recompute the matches for `query` (sync; our lists are small).
    fn update_matches(&mut self, query: &str, window: &mut Window, cx: &mut Context<Picker<Self>>);
    fn render_match(&self, ix: usize, selected: bool, cx: &mut Context<Picker<Self>>) -> AnyElement;
    /// Run match `ix`; [`Confirmed::Dismiss`] closes, [`Confirmed::Continue`]
    /// starts the next stage.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> Confirmed;

    /// Badges rendered inside the input row, before the query (e.g. the chosen
    /// command and collected arguments in a multi-stage flow).
    fn render_prefix(&self, _cx: &mut Context<Picker<Self>>) -> Option<AnyElement> {
        None
    }

    /// Backspace on an empty query: step back a stage. Return `true` if a stage
    /// was popped (the picker then starts a fresh stage), `false` to ignore.
    fn back(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) -> bool {
        false
    }

    /// Whether results are still loading — shows a spinner in the input row.
    fn is_loading(&self) -> bool {
        false
    }
}

pub struct Picker<D: PickerDelegate> {
    pub delegate: D,
    query: Entity<TextInput>,
    last_query: String,
    /// Whether the delegate was loading at the previous render — lets us refilter
    /// on the frame an async fetch completes without refiltering every render.
    was_loading: bool,
    scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    _query_subs: [Subscription; 2],
}

impl<D: PickerDelegate> EventEmitter<DismissEvent> for Picker<D> {}

impl<D: PickerDelegate> Focusable for Picker<D> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl<D: PickerDelegate> Picker<D> {
    pub fn new(mut delegate: D, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| TextInput::new(cx, delegate.placeholder()).bare());
        // Re-render (and thus re-filter) whenever the query is edited.
        let edit_sub = cx.observe(&query, |_, _, cx| cx.notify());
        // Backspace on an empty query steps back a stage.
        let back_sub = cx.subscribe_in(&query, window, |this, _, event: &TextInputEvent, window, cx| {
            let TextInputEvent::BackspaceOnEmpty = event;
            if this.delegate.back(window, cx) {
                this.reset_for_stage(window, cx);
            }
        });
        delegate.update_matches("", window, cx);
        query.read(cx).focus_handle(cx).focus(window, cx);
        Self {
            delegate,
            query,
            last_query: String::new(),
            was_loading: false,
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            _query_subs: [edit_sub, back_sub],
        }
    }

    /// Start a fresh stage: clear the query, re-read the placeholder, refilter.
    fn reset_for_stage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = self.delegate.placeholder();
        self.query.update(cx, |q, cx| {
            q.set_text("", cx);
            q.set_placeholder(placeholder, cx);
        });
        self.last_query = String::new();
        self.delegate.update_matches("", window, cx);
        self.delegate.set_selected_index(0, cx);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn move_selection(&mut self, delta: i32, cx: &mut Context<Self>) {
        let count = self.delegate.match_count();
        if count == 0 {
            return;
        }
        let cur = self.delegate.selected_index().min(count - 1) as i32;
        let next = (cur + delta).rem_euclid(count as i32) as usize;
        self.delegate.set_selected_index(next, cx);
        self.scroll.scroll_to_item(next, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(-1, cx);
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(1, cx);
    }

    fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        if self.delegate.match_count() == 0 {
            return;
        }
        let ix = self.delegate.selected_index();
        self.confirm_index(ix, window, cx);
    }

    fn confirm_index(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        match self.delegate.confirm(ix, window, cx) {
            Confirmed::Dismiss => cx.emit(DismissEvent),
            Confirmed::Continue => self.reset_for_stage(window, cx),
            // Replace the query; the edit re-renders and refilters (descend).
            Confirmed::SetQuery(q) => self.query.update(cx, |inp, cx| inp.set_text(q, cx)),
        }
    }
}

impl<D: PickerDelegate> Render for Picker<D> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Recompute matches when the query changed, or while/just-after an async
        // completion-source fetch (so results appear) — not on every render, so
        // plain navigation doesn't re-run the filter.
        let query = self.query.read(cx).text().to_string();
        let changed = query != self.last_query;
        let loading = self.delegate.is_loading();
        if changed || loading || self.was_loading {
            self.delegate.update_matches(&query, window, cx);
            if changed {
                self.last_query = query;
                self.delegate.set_selected_index(0, cx);
                self.scroll.scroll_to_item(0, ScrollStrategy::Top);
            }
        }
        self.was_loading = loading;
        let count = self.delegate.match_count();
        v_flex()
            .id("picker")
            // "modal" suppresses the workspace's `!modal` shortcuts; "Picker" scopes
            // the nav keys to a focused field inside.
            .key_context("modal Picker")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            // Swallow clicks so they don't fall through to the dismiss backdrop.
            .on_mouse_down(MouseButton::Left, cx.listener(|_, _: &MouseDownEvent, _, cx| cx.stop_propagation()))
            .w(px(544.0))
            .rounded_lg()
            .bg(rgb(ELEVATED))
            .border_1()
            .border_color(rgb(BORDER_MUTED))
            .shadow_lg()
            .overflow_hidden()
            .child(
                h_flex()
                    .h(px(36.0))
                    .px(px(10.0))
                    .gap_2()
                    .items_center()
                    .children(self.delegate.render_prefix(cx))
                    .child(div().flex_grow(1.0).child(self.query.clone()))
                    .when(self.delegate.is_loading(), |el| {
                        el.child(spinner("picker-loading", px(14.0), FG_MUTED))
                    }),
            )
            .child(divider())
            .when(count > 0, |el| {
                el.child(
                    v_flex().flex_grow(1.0).max_h(px(384.0)).overflow_hidden().child(
                        uniform_list(
                            "picker-list",
                            count,
                            cx.processor(|this, range: Range<usize>, _window, cx| {
                                let sel = this.delegate.selected_index();
                                range
                                    .map(|ix| {
                                        div()
                                            .id(("picker-row", ix))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                cx.stop_propagation();
                                                window.prevent_default();
                                                this.confirm_index(ix, window, cx)
                                            }))
                                            .child(this.delegate.render_match(ix, ix == sel, cx))
                                            .into_any_element()
                                    })
                                    .collect()
                            }),
                        )
                        .track_scroll(&self.scroll)
                        .with_sizing_behavior(ListSizingBehavior::Infer)
                        .flex_grow(1.0)
                        .px(px(4.0))
                        .py_1(),
                    ),
                )
            })
    }
}
