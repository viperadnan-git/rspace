//! Custom [`gpui::Element`] that shapes and paints the input's text, caret,
//! and selection (see [`super::TextInput`]).

use super::*;

pub(super) struct TextElement {
    pub(super) input: Entity<TextInput>,
}

pub(super) struct Prepaint {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    origin: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepaint {
        let input = self.input.read(cx);
        let style = window.text_style();
        let cursor = input.cursor();
        let selected = input.selected_range.clone();

        let (display, color): (SharedString, _) = if input.content.is_empty() {
            (input.placeholder.clone(), rgb(FG_SUBTLE).into())
        } else if input.masked {
            ("•".repeat(input.content.chars().count()).into(), style.color)
        } else {
            (input.content.clone(), style.color)
        };

        let run = TextRun {
            len: display.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window.text_system().shape_line(display, font_size, &[run], None);

        // Map a content byte offset onto the (possibly masked) shaped line.
        let masked = input.masked && !input.content.is_empty();
        let shaped_index = |offset: usize| -> usize {
            if masked {
                input.content[..offset].chars().count() * "•".len()
            } else {
                offset
            }
        };

        // Single-line horizontal scroll: keep the caret inside the field. Scroll
        // only when the caret would fall outside the visible window, then clamp
        // so the text never scrolls past its start.
        let caret_x = line.x_for_index(shaped_index(cursor));
        let pad = px(2.0);
        let prev = input.scroll_offset;
        let mut scroll = prev;
        if caret_x < scroll {
            scroll = caret_x;
        } else if caret_x > scroll + bounds.size.width - pad {
            scroll = caret_x - bounds.size.width + pad;
        }
        scroll = scroll.max(px(0.0)).min((line.width - bounds.size.width + pad).max(px(0.0)));
        // Center the (fitting) text when requested; otherwise left-align + scroll.
        let align = if input.centered {
            ((bounds.size.width - line.width) / 2.0).max(px(0.0))
        } else {
            px(0.0)
        };
        let origin = bounds.left() + align - scroll;

        let (cursor, selection) = if selected.is_empty() {
            (
                Some(fill(
                    Bounds::new(point(origin + caret_x, bounds.top()), size(px(1.5), bounds.bottom() - bounds.top())),
                    rgb(ACCENT),
                )),
                None,
            )
        } else {
            (
                None,
                Some(fill(
                    Bounds::from_corners(
                        point(origin + line.x_for_index(shaped_index(selected.start)), bounds.top()),
                        point(origin + line.x_for_index(shaped_index(selected.end)), bounds.bottom()),
                    ),
                    rgba(SELECT),
                )),
            )
        };
        // `input` (the read borrow) is no longer used past here, so persisting the
        // new scroll offset can take a mutable borrow.
        if scroll != prev {
            self.input.update(cx, |input, _| input.scroll_offset = scroll);
        }
        Prepaint { line: Some(line), cursor, selection, origin }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus_handle.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.input.clone()), cx);
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let line = prepaint.line.take().unwrap();
        let text_origin = point(prepaint.origin, bounds.top());
        line.paint(text_origin, window.line_height(), gpui::TextAlign::Left, None, window, cx).ok();
        if focus.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}
