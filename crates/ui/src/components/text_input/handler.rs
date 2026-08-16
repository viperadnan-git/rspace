//! IME / input-method plumbing: `EntityInputHandler`, `Focusable`, events.

use super::*;

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
        let range = self.range_from_utf16(&range_utf16);
        // The painted origin, not the field's left edge.
        let origin_x = bounds.left() + self.align_offset() - self.scroll_offset;
        let line = self.last_layout.as_ref()?;
        Some(Bounds::from_corners(
            point(origin_x + line.x_for_index(range.start), bounds.top()),
            point(origin_x + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        // Window x -> offset within the shaped line, as `index_for_position`.
        let bounds = self.last_bounds?;
        let local = point.x - bounds.left() - self.align_offset() + self.scroll_offset;
        let i = self.last_layout.as_ref()?.index_for_x(local)?;
        Some(self.offset_to_utf16(i))
    }
}
