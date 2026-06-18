//! A reusable single-line text input: real caret, selection, mouse, and IME via
//! `EntityInputHandler`, modeled on gpui's input example. Used wherever the app
//! needs typed text (config forms, etc.).

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, rgb, rgba, size, svg, App, Bounds,
    ClickEvent, ClipboardItem, Context, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, LayoutId,
    MouseDownEvent, MouseMoveEvent, PaintQuad, Pixels, Point, ShapedLine, SharedString, Style,
    TextRun, UTF16Selection, Window,
};

use crate::theme::*;

actions!(
    text_input,
    [
        Left,
        Right,
        WordLeft,
        WordRight,
        SelectLeft,
        SelectRight,
        SelectWordLeft,
        SelectWordRight,
        SelectAll,
        Home,
        End,
        SelectToHome,
        SelectToEnd,
        Backspace,
        Delete,
        DeleteWordBack,
        DeleteWordForward,
        DeleteToStart,
        DeleteToEnd,
        Paste,
        Copy,
        Cut
    ]
);

/// Events a host can subscribe to. Currently just the backspace-on-empty signal
/// used by the multi-stage picker to step back a stage.
pub enum TextInputEvent {
    BackspaceOnEmpty,
}

/// Character class for word motion (mirrors Zed's `CharKind`): word chars,
/// punctuation, and whitespace are distinct, so word motion stops at word⇄
/// punctuation transitions, not only at spaces.
#[derive(PartialEq, Clone, Copy)]
enum CharKind {
    Whitespace,
    Punctuation,
    Word,
}

fn char_kind(c: char) -> CharKind {
    if c.is_alphanumeric() || c == '_' {
        CharKind::Word
    } else if c.is_whitespace() {
        CharKind::Whitespace
    } else {
        CharKind::Punctuation
    }
}

pub fn bind_keys(cx: &mut App) {
    use gpui::KeyBinding as K;
    let ctx = Some("TextInput");
    // Portable across OSes. `secondary-` is cmd on macOS, ctrl elsewhere.
    let mut binds = vec![
        K::new("left", Left, ctx),
        K::new("right", Right, ctx),
        K::new("shift-left", SelectLeft, ctx),
        K::new("shift-right", SelectRight, ctx),
        K::new("home", Home, ctx),
        K::new("end", End, ctx),
        K::new("shift-home", SelectToHome, ctx),
        K::new("shift-end", SelectToEnd, ctx),
        K::new("backspace", Backspace, ctx),
        K::new("delete", Delete, ctx),
        K::new("secondary-a", SelectAll, ctx),
        K::new("secondary-c", Copy, ctx),
        K::new("secondary-v", Paste, ctx),
        K::new("secondary-x", Cut, ctx),
    ];
    // Word/line motions use the platform's native modifiers: Option + Cmd on
    // macOS, Ctrl elsewhere.
    #[cfg(target_os = "macos")]
    binds.extend([
        K::new("alt-left", WordLeft, ctx),
        K::new("alt-right", WordRight, ctx),
        K::new("alt-shift-left", SelectWordLeft, ctx),
        K::new("alt-shift-right", SelectWordRight, ctx),
        K::new("alt-backspace", DeleteWordBack, ctx),
        K::new("alt-delete", DeleteWordForward, ctx),
        K::new("cmd-left", Home, ctx),
        K::new("cmd-right", End, ctx),
        K::new("cmd-shift-left", SelectToHome, ctx),
        K::new("cmd-shift-right", SelectToEnd, ctx),
        K::new("cmd-backspace", DeleteToStart, ctx),
        K::new("cmd-delete", DeleteToEnd, ctx),
    ]);
    #[cfg(not(target_os = "macos"))]
    binds.extend([
        K::new("ctrl-left", WordLeft, ctx),
        K::new("ctrl-right", WordRight, ctx),
        K::new("ctrl-shift-left", SelectWordLeft, ctx),
        K::new("ctrl-shift-right", SelectWordRight, ctx),
        K::new("ctrl-backspace", DeleteWordBack, ctx),
        K::new("ctrl-delete", DeleteWordForward, ctx),
    ]);
    cx.bind_keys(binds);
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
    /// Horizontal scroll so the caret stays visible when the text is wider than
    /// the field (single-line scroll, like a native text input).
    scroll_offset: Pixels,
    /// Center the text within the field (for compact values like a stepper).
    centered: bool,
    /// Show an inline clear (×) button when focused and non-empty (search fields).
    clearable: bool,
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
            scroll_offset: px(0.0),
            centered: false,
            clearable: false,
        }
    }

    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    pub fn centered(mut self) -> Self {
        self.centered = true;
        self
    }

    pub fn clearable(mut self) -> Self {
        self.clearable = true;
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

    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = placeholder.into();
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

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.prev_word_boundary(self.cursor()), cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.next_word_boundary(self.cursor()), cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.prev_boundary(self.cursor()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor()), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.prev_word_boundary(self.cursor()), cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_word_boundary(self.cursor()), cx);
    }

    fn select_to_home(&mut self, _: &SelectToHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_to_end(&mut self, _: &SelectToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
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
        // Backspace on an already-empty field is a "go back" signal for hosts
        // like the multi-stage picker.
        if self.content.is_empty() {
            cx.emit(TextInputEvent::BackspaceOnEmpty);
            return;
        }
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

    fn delete_word_back(&mut self, _: &DeleteWordBack, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.prev_word_boundary(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_forward(&mut self, _: &DeleteWordForward, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_word_boundary(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_to_start(&mut self, _: &DeleteToStart, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(0, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_to_end(&mut self, _: &DeleteToEnd, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.content.len(), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Consume the click so it doesn't reach a backdrop that blurs on outside
        // clicks, and focus explicitly (stopping propagation suppresses gpui's
        // focus-on-click) — clicking into the field should focus it.
        cx.stop_propagation();
        self.focus_handle.focus(window, cx);
        let offset = self.index_for_x(ev.position);
        match ev.click_count {
            // Double-click selects the word under the cursor; triple selects all.
            2 => {
                let (start, end) = self.word_at(offset);
                self.selection_reversed = false;
                self.selected_range = start..end;
                cx.notify();
            }
            n if n >= 3 => {
                self.selection_reversed = false;
                self.selected_range = 0..self.content.len();
                cx.notify();
            }
            _ if ev.modifiers.shift => self.select_to(offset, cx),
            _ => self.move_to(offset, cx),
        }
    }

    /// The maximal run of word characters (alphanumerics, `_-.`) around `offset`.
    fn word_at(&self, offset: usize) -> (usize, usize) {
        let is_word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
        let start = self.content[..offset]
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_word(*c))
            .last()
            .map_or(offset, |(i, _)| i);
        let end = self.content[offset..]
            .char_indices()
            .take_while(|(_, c)| is_word(*c))
            .last()
            .map_or(offset, |(i, c)| offset + i + c.len_utf8());
        (start, end)
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

    /// Empty the field (the inline clear button) and keep focus for more typing.
    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.content = SharedString::default();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.scroll_offset = px(0.0);
        self.error = None;
        self.focus_handle.focus(window, cx);
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
        // The placeholder is shown but isn't editable text, so a click collapses
        // to the start rather than landing at a placeholder offset (which would
        // be out of bounds for the empty content).
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref()) else {
            return 0;
        };
        let local = position.x - bounds.left() - self.align_offset() + self.scroll_offset;
        let Some(offset) = line.index_for_x(local) else {
            return self.content.len();
        };
        if self.masked {
            // The shaped line is one "•" (3 bytes) per char; map back to content.
            let chars = offset / "\u{2022}".len();
            self.content.char_indices().nth(chars).map_or(self.content.len(), |(b, _)| b)
        } else {
            self.clamp_content(offset)
        }
    }

    /// When centered and the text fits the field, the x offset that centers it.
    fn align_offset(&self) -> Pixels {
        if !self.centered {
            return px(0.0);
        }
        match (self.last_bounds.as_ref(), self.last_layout.as_ref()) {
            (Some(bounds), Some(line)) => ((bounds.size.width - line.width) / 2.0).max(px(0.0)),
            _ => px(0.0),
        }
    }

    /// Clamp a content byte offset into range and onto a char boundary, so it is
    /// always safe to slice `content` with it.
    fn clamp_content(&self, offset: usize) -> usize {
        let len = self.content.len();
        if offset >= len {
            return len;
        }
        let mut o = offset;
        while o > 0 && !self.content.is_char_boundary(o) {
            o -= 1;
        }
        o
    }

    /// Both ends clamped into range and ordered, so it's safe to splice `content`.
    fn clamp_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self.clamp_content(range.start);
        let end = self.clamp_content(range.end).max(start);
        start..end
    }

    fn prev_boundary(&self, offset: usize) -> usize {
        self.content[..offset].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content[offset..].char_indices().nth(1).map_or(self.content.len(), |(i, _)| offset + i)
    }

    /// Byte offset of the previous word boundary (Zed's model: skip whitespace,
    /// then consume one run of a single [`CharKind`] — so motion stops at
    /// word⇄punctuation transitions, not just spaces).
    fn prev_word_boundary(&self, offset: usize) -> usize {
        let chars: Vec<(usize, char)> = self.content[..offset].char_indices().collect();
        let mut i = chars.len();
        while i > 0 && char_kind(chars[i - 1].1) == CharKind::Whitespace {
            i -= 1;
        }
        if i > 0 {
            let kind = char_kind(chars[i - 1].1);
            while i > 0 && char_kind(chars[i - 1].1) == kind {
                i -= 1;
            }
        }
        chars.get(i).map_or(offset, |(b, _)| *b)
    }

    /// Byte offset of the next word boundary (mirror of [`Self::prev_word_boundary`]).
    fn next_word_boundary(&self, offset: usize) -> usize {
        let chars: Vec<(usize, char)> = self.content[offset..].char_indices().collect();
        let mut i = 0;
        while i < chars.len() && char_kind(chars[i].1) == CharKind::Whitespace {
            i += 1;
        }
        if i < chars.len() {
            let kind = char_kind(chars[i].1);
            while i < chars.len() && char_kind(chars[i].1) == kind {
                i += 1;
            }
        }
        chars.get(i).map_or(self.content.len(), |(b, _)| offset + b)
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

impl EventEmitter<TextInputEvent> for TextInput {}

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
        let Range { start, end } = self.clamp_range(range);
        self.content = (self.content[..start].to_owned() + new_text + &self.content[end..]).into();
        let at = start + new_text.len();
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
        let Range { start, end } = self.clamp_range(range);
        self.content = (self.content[..start].to_owned() + new_text + &self.content[end..]).into();
        self.marked_range = (!new_text.is_empty()).then(|| start..start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .map(|r| self.range_from_utf16(&r))
            .map(|r| r.start + start..r.end + start)
            .unwrap_or_else(|| {
                let at = start + new_text.len();
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


mod element;
use element::TextElement;
