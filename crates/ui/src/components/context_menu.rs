//! The one focusable primitive every menu renders through, so keyboard
//! navigation belongs to the component (Zed's `ContextMenu`).
//!
//! Callers describe rows with [`MenuSpec`]; this owns the focus handle, the
//! selected row, and the bindings under the `Menu` key context.

use std::rc::Rc;

use gpui::{DismissEvent, EventEmitter, FocusHandle, Focusable};

use super::*;

actions!(menu, [MenuNext, MenuPrev, MenuConfirm, MenuCancel]);

/// A handler bound to a menu row; runs against the workspace when chosen.
pub(crate) type MenuAction = Rc<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

pub(crate) enum MenuRow {
    Item { id: gpui::ElementId, label: SharedString, icon: &'static str, danger: bool, action: MenuAction },
    Separator,
}

impl MenuRow {
    fn is_item(&self) -> bool {
        matches!(self, MenuRow::Item { .. })
    }
}

/// A non-selectable block above the rows: status for the daemon popover.
pub(crate) struct MenuHeader {
    pub(crate) icon: &'static str,
    pub(crate) tint: u32,
    pub(crate) title: SharedString,
    pub(crate) subtitle: SharedString,
    pub(crate) error: Option<SharedString>,
}

/// A declarative context menu, built fluently. Rows stay data rather than
/// pre-rendered elements, so callers compose from the live selection while
/// [`ContextMenu`] owns styling, selection and keys.
#[derive(Default)]
pub(crate) struct MenuSpec {
    pub(crate) rows: Vec<MenuRow>,
    pub(crate) header: Option<MenuHeader>,
}

impl MenuSpec {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn row(
        mut self,
        id: impl Into<gpui::ElementId>,
        label: impl Into<SharedString>,
        icon: &'static str,
        danger: bool,
        action: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
    ) -> Self {
        self.rows.push(MenuRow::Item {
            id: id.into(),
            label: label.into(),
            icon,
            danger,
            action: Rc::new(action),
        });
        self
    }

    pub(crate) fn item(
        self,
        id: impl Into<gpui::ElementId>,
        label: impl Into<SharedString>,
        icon: &'static str,
        action: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
    ) -> Self {
        self.row(id, label, icon, false, action)
    }

    pub(crate) fn danger(
        self,
        id: impl Into<gpui::ElementId>,
        label: impl Into<SharedString>,
        icon: &'static str,
        action: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
    ) -> Self {
        self.row(id, label, icon, true, action)
    }

    pub(crate) fn header(mut self, header: MenuHeader) -> Self {
        self.header = Some(header);
        self
    }

    /// Append `f`'s rows only when `cond` holds — the conditional-row primitive.
    pub(crate) fn when(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond { f(self) } else { self }
    }

    /// A group boundary, drawn as a divider only when both sides have items, so
    /// callers add one between optional groups without tracking which emitted.
    pub(crate) fn separator(mut self) -> Self {
        self.rows.push(MenuRow::Separator);
        self
    }
}

pub(crate) struct ContextMenu {
    workspace: WeakEntity<Workspace>,
    header: Option<MenuHeader>,
    rows: Vec<MenuRow>,
    /// Index into `rows` of the highlighted item; `None` until the user moves.
    selected: Option<usize>,
    focus_handle: FocusHandle,
    focused: bool,
}

impl EventEmitter<DismissEvent> for ContextMenu {}

impl Focusable for ContextMenu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ContextMenu {
    pub(crate) fn new(spec: MenuSpec, workspace: WeakEntity<Workspace>, cx: &mut Context<Self>) -> Self {
        // Open on the first item, per the ARIA menu-button pattern: focus
        // belongs on a row, not the container.
        let selected = spec.rows.iter().position(MenuRow::is_item);
        Self {
            workspace,
            header: spec.header,
            rows: spec.rows,
            selected,
            focus_handle: cx.focus_handle(),
            focused: false,
        }
    }

    /// Indices of the selectable rows, in display order (separators skipped).
    fn item_indices(&self) -> Vec<usize> {
        self.rows.iter().enumerate().filter(|(_, r)| r.is_item()).map(|(i, _)| i).collect()
    }

    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let items = self.item_indices();
        if items.is_empty() {
            return;
        }
        let at = self.selected.and_then(|s| items.iter().position(|&i| i == s));
        let next = match (at, forward) {
            (Some(p), true) => (p + 1) % items.len(),
            (Some(p), false) => (p + items.len() - 1) % items.len(),
            (None, true) => 0,
            (None, false) => items.len() - 1,
        };
        self.selected = Some(items[next]);
        cx.notify();
    }

    fn select_next(&mut self, _: &MenuNext, _: &mut Window, cx: &mut Context<Self>) {
        self.step(true, cx);
    }

    fn select_prev(&mut self, _: &MenuPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.step(false, cx);
    }

    fn cancel(&mut self, _: &MenuCancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn confirm(&mut self, _: &MenuConfirm, window: &mut Window, cx: &mut Context<Self>) {
        let Some(action) = self.selected.and_then(|i| self.action_at(i)) else {
            return;
        };
        self.run(action, window, cx);
    }

    fn action_at(&self, index: usize) -> Option<MenuAction> {
        match self.rows.get(index) {
            Some(MenuRow::Item { action, .. }) => Some(action.clone()),
            _ => None,
        }
    }

    /// Run a row's action against the workspace, then dismiss.
    fn run(&mut self, action: MenuAction, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.update(cx, |ws, cx| action(ws, window, cx)).ok();
        cx.emit(DismissEvent);
    }
}

impl Render for ContextMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The openers have no `Window`, so focus is claimed here — once, or it
        // would trap focus every frame.
        focus_once(&mut self.focused, &self.focus_handle, window, cx);

        // Resolve separators structurally: a boundary becomes a divider only with
        // an item on both sides, so one never leads, trails, or doubles up no
        // matter which conditional groups are empty.
        let mut items: Vec<AnyElement> = Vec::with_capacity(self.rows.len() + 2);
        if let Some(h) = &self.header {
            items.push(
                v_flex()
                    .w_full()
                    .px_2()
                    .py_1()
                    .gap(px(2.0))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(svg().path(h.icon).size(rem(14.0)).flex_shrink_0().text_color(rgb(h.tint)))
                            .child(div().text_color(rgb(FG)).child(h.title.clone())),
                    )
                    .child(div().text_xs().text_color(rgb(FG_MUTED)).child(h.subtitle.clone()))
                    .when_some(h.error.clone(), |el, e| {
                        el.child(div().text_xs().text_color(rgb(DANGER)).child(e))
                    })
                    .into_any_element(),
            );
        }
        let mut pending_divider = self.header.is_some();
        for (index, row) in self.rows.iter().enumerate() {
            match row {
                MenuRow::Separator => pending_divider = !items.is_empty(),
                MenuRow::Item { id, label, icon, danger, action } => {
                    if std::mem::take(&mut pending_divider) {
                        items.push(div().my_1().h(px(1.0)).bg(rgb(BORDER_MUTED)).into_any_element());
                    }
                    let (text, icon_color) = if *danger { (DANGER, DANGER) } else { (FG, FG_MUTED) };
                    let action = action.clone();
                    items.push(
                        h_flex()
                            .id(id.clone())
                            .w_full()
                            .gap_2()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_color(rgb(text))
                            .when(self.selected == Some(index), |el| el.bg(rgba(SELECT)))
                            .hover(|s| s.bg(rgba(OVERLAY)))
                            // Keep the keyboard cursor with the pointer.
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered && this.selected != Some(index) {
                                    this.selected = Some(index);
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.run(action.clone(), window, cx);
                            }))
                            .child(svg().path(*icon).size(rem(15.0)).flex_shrink_0().text_color(rgb(icon_color)))
                            .child(label.clone())
                            .into_any_element(),
                    );
                }
            }
        }

        v_flex()
            .key_context("Menu")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .occlude()
            .min_w(rem(180.0))
            .p_1()
            .rounded_md()
            .bg(rgb(ELEVATED))
            .border_1()
            .border_color(rgb(BORDER_MUTED))
            .shadow_lg()
            .text_color(rgb(FG))
            .on_mouse_down_out(cx.listener(|_, _: &MouseDownEvent, _, cx| cx.emit(DismissEvent)))
            .children(items)
    }
}
