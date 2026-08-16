//! gpui drag payloads and resize targets — the values handed to `.on_drag()` for
//! pane/column resizing, tab reordering, file-list drags, and the marquee. Most
//! carry no state and render nothing; [`DragLabel`] paints the drag preview.

use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ResizeTarget {
    Sidebar,
    Dock,
    /// The divider between the two panes of a split (adjusts the split ratio).
    PaneSplit,
}

#[derive(Clone)]
pub(crate) struct DragResize(pub(crate) ResizeTarget);

impl Render for DragResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Column {
    Date,
    Size,
}

/// `owner` is the explorer that started the drag: gpui fires `on_drag_move` on
/// every mounted explorer, so each ignores events whose owner isn't itself.
#[derive(Clone)]
pub(crate) struct DragColumn {
    pub(crate) col: Column,
    pub(crate) owner: gpui::EntityId,
}

impl Render for DragColumn {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// Rubber-band selection in the file list. The band's anchor and live selection
/// live on the [`Explorer`]; `owner` scopes the drag to the explorer that started
/// it (gpui fires `on_drag_move` on every mounted explorer).
#[derive(Clone)]
pub(crate) struct DragMarquee {
    pub(crate) owner: gpui::EntityId,
}

impl Render for DragMarquee {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

pub(crate) struct DraggedRemote {
    pub(crate) name: String,
}

/// A tab being dragged to reorder it within the strip.
#[derive(Clone)]
pub(crate) struct DraggedTab {
    pub(crate) id: usize,
    pub(crate) title: SharedString,
}

/// One entry inside a [`DraggedEntry`].
#[derive(Clone)]
pub(crate) struct DragItem {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) is_dir: bool,
}

/// A drag from the file list — self-contained so a drop is correct anywhere,
/// regardless of the active tab. `remote` and `items` are snapshotted at drag
/// start: `items` is the whole selection (or the single dragged row), each with
/// its full path.
#[derive(Clone)]
pub(crate) struct DraggedEntry {
    pub(crate) remote: String,
    pub(crate) items: Vec<DragItem>,
}

pub(crate) struct DragLabel {
    pub(crate) text: SharedString,
    /// Cursor position within the grabbed element at drag start. gpui paints the
    /// drag preview at `cursor - offset`, so shifting the label back by `offset`
    /// re-anchors it to the cursor regardless of where a wide row was grabbed.
    pub(crate) offset: Point<Pixels>,
}

impl DragLabel {
    pub(crate) fn new(text: impl Into<SharedString>, offset: Point<Pixels>) -> Self {
        Self { text: text.into(), offset }
    }
}

impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().pl(self.offset.x + px(12.0)).pt(self.offset.y + px(8.0)).child(
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(rgb(ELEVATED))
                .shadow_lg()
                .text_xs()
                .text_color(rgb(FG))
                .child(self.text.clone()),
        )
    }
}
