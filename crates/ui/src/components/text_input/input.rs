//! `impl TextInput`: cursor motion, selection, editing, word/UTF-16 helpers.

use super::*;

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

    pub(super) fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(if self.selected_range.is_empty() {
            self.prev_boundary(self.cursor())
        } else {
            self.selected_range.start
        }, cx);
    }

    pub(super) fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(if self.selected_range.is_empty() {
            self.next_boundary(self.cursor())
        } else {
            self.selected_range.end
        }, cx);
    }

    pub(super) fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.prev_word_boundary(self.cursor()), cx);
    }

    pub(super) fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.next_word_boundary(self.cursor()), cx);
    }

    pub(super) fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.prev_boundary(self.cursor()), cx);
    }

    pub(super) fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor()), cx);
    }

    pub(super) fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.prev_word_boundary(self.cursor()), cx);
    }

    pub(super) fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_word_boundary(self.cursor()), cx);
    }

    pub(super) fn select_to_home(&mut self, _: &SelectToHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    pub(super) fn select_to_end(&mut self, _: &SelectToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    pub(super) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    pub(super) fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    pub(super) fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    pub(super) fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(super) fn delete_word_back(&mut self, _: &DeleteWordBack, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.prev_word_boundary(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(super) fn delete_word_forward(&mut self, _: &DeleteWordForward, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_word_boundary(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(super) fn delete_to_start(&mut self, _: &DeleteToStart, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(0, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(super) fn delete_to_end(&mut self, _: &DeleteToEnd, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.content.len(), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(super) fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(super) fn word_at(&self, offset: usize) -> (usize, usize) {
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

    pub(super) fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if ev.pressed_button == Some(gpui::MouseButton::Left) {
            self.select_to(self.index_for_x(ev.position), cx);
        }
    }

    pub(super) fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    pub(super) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
        }
    }

    pub(super) fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    pub(super) fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify();
    }

    /// Empty the field (the inline clear button) and keep focus for more typing.
    pub(super) fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.content = SharedString::default();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.scroll_offset = px(0.0);
        self.error = None;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    pub(super) fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    pub(super) fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
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

    pub(super) fn index_for_x(&self, position: Point<Pixels>) -> usize {
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
    pub(super) fn align_offset(&self) -> Pixels {
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
    pub(super) fn clamp_content(&self, offset: usize) -> usize {
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
    pub(super) fn clamp_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self.clamp_content(range.start);
        let end = self.clamp_content(range.end).max(start);
        start..end
    }

    pub(super) fn prev_boundary(&self, offset: usize) -> usize {
        self.content[..offset].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    pub(super) fn next_boundary(&self, offset: usize) -> usize {
        self.content[offset..].char_indices().nth(1).map_or(self.content.len(), |(i, _)| offset + i)
    }

    /// Byte offset of the previous word boundary (Zed's model: skip whitespace,
    /// then consume one run of a single [`CharKind`] — so motion stops at
    /// word⇄punctuation transitions, not just spaces).
    pub(super) fn prev_word_boundary(&self, offset: usize) -> usize {
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
    pub(super) fn next_word_boundary(&self, offset: usize) -> usize {
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

    pub(super) fn offset_to_utf16(&self, offset: usize) -> usize {
        self.content[..offset].chars().map(char::len_utf16).sum()
    }

    pub(super) fn offset_from_utf16(&self, target: usize) -> usize {
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

    pub(super) fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub(super) fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}
