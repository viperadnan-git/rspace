//! A reusable single-line text input: real caret, selection, mouse, and IME via
//! `EntityInputHandler`, modeled on gpui's input example. Used wherever the app
//! needs typed text (config forms, etc.).

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, rgb, rgba, size, App, Bounds,
    ClipboardItem, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    FocusHandle, Focusable, GlobalElementId, LayoutId, MouseDownEvent, MouseMoveEvent,
    PaintQuad, Pixels, Point, ShapedLine, SharedString, Style, TextRun, UTF16Selection, Window,
};

use crate::theme::*;

actions!(
    text_input,
    [Left, Right, SelectLeft, SelectRight, SelectAll, Home, End, Backspace, Delete, Paste, Copy, Cut]
);

pub fn bind_keys(cx: &mut App) {
    let ctx = Some("TextInput");
    cx.bind_keys([
        gpui::KeyBinding::new("left", Left, ctx),
        gpui::KeyBinding::new("right", Right, ctx),
        gpui::KeyBinding::new("shift-left", SelectLeft, ctx),
        gpui::KeyBinding::new("shift-right", SelectRight, ctx),
        gpui::KeyBinding::new("cmd-a", SelectAll, ctx),
        gpui::KeyBinding::new("home", Home, ctx),
        gpui::KeyBinding::new("end", End, ctx),
        gpui::KeyBinding::new("backspace", Backspace, ctx),
        gpui::KeyBinding::new("delete", Delete, ctx),
        gpui::KeyBinding::new("cmd-v", Paste, ctx),
        gpui::KeyBinding::new("cmd-c", Copy, ctx),
        gpui::KeyBinding::new("cmd-x", Cut, ctx),
    ]);
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    masked: bool,
    /// No box chrome (bg/border/padding) — for embedding inline (e.g. a list row).
    bare: bool,
    /// Validation error: red border + message below; cleared on edit.
    error: Option<SharedString>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
}

impl TextInput {
    pub fn new(cx: &mut Context<Self>, placeholder: impl Into<SharedString>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: placeholder.into(),
            masked: false,
            bare: false,
            error: None,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
        }
    }

    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.error = None;
        cx.notify();
    }

    /// Set or clear the field-level validation error.
    pub fn set_error(&mut self, error: Option<SharedString>, cx: &mut Context<Self>) {
        self.error = error;
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(if self.selected_range.is_empty() {
            self.prev_boundary(self.cursor())
        } else {
            self.selected_range.start
        }, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(if self.selected_range.is_empty() {
            self.next_boundary(self.cursor())
        } else {
            self.selected_range.end
        }, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.prev_boundary(self.cursor()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.prev_boundary(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.index_for_x(ev.position);
        if ev.modifiers.shift {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if ev.pressed_button == Some(gpui::MouseButton::Left) {
            self.select_to(self.index_for_x(ev.position), cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify();
    }

    fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn index_for_x(&self, position: Point<Pixels>) -> usize {
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref()) else {
            return 0;
        };
        line.index_for_x(position.x - bounds.left()).unwrap_or(self.content.len())
    }

    fn prev_boundary(&self, offset: usize) -> usize {
        self.content[..offset].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content[offset..].char_indices().nth(1).map_or(self.content.len(), |(i, _)| offset + i)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        self.content[..offset].chars().map(char::len_utf16).sum()
    }

    fn offset_from_utf16(&self, target: usize) -> usize {
        let (mut utf16, mut utf8) = (0, 0);
        for ch in self.content.chars() {
            if utf16 >= target {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|r| self.range_from_utf16(&r))
            .or_else(|| self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content = (self.content[..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        let at = range.start + new_text.len();
        self.selected_range = at..at;
        self.marked_range = None;
        self.error = None;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|r| self.range_from_utf16(&r))
            .or_else(|| self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content = (self.content[..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        self.marked_range = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .map(|r| self.range_from_utf16(&r))
            .map(|r| r.start + range.start..r.end + range.start)
            .unwrap_or_else(|| {
                let at = range.start + new_text.len();
                at..at
            });
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start), bounds.top()),
            point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let line = self.last_layout.as_ref()?;
        let i = line.index_for_x(point.x - line_point.x)?;
        Some(self.offset_to_utf16(i))
    }
}

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
        let input_box = div()
            .track_focus(&self.focus_handle)
            .key_context("TextInput")
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, w, cx| this.on_mouse_down(ev, w, cx)))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, w, cx| this.on_mouse_move(ev, w, cx)))
            // A tab stop, so forms get keyboard navigation automatically.
            .tab_index(0)
            .tab_stop(true)
            .w_full()
            .text_sm()
            .line_height(px(18.0))
            .cursor_text()
            // Box chrome, unless embedded inline (e.g. a list row supplies its own).
            .when(!self.bare, |el| {
                el.px_2()
                    .py(px(5.0))
                    .rounded_md()
                    .bg(rgb(INSET))
                    .border_1()
                    .border_color(rgb(border))
            })
            .child(TextElement { input: cx.entity() });
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


mod element;
use element::TextElement;
