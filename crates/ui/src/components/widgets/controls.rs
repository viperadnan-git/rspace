//! Interactive form controls (`RenderOnce` + builder + stored `on_click`, the
//! Zed pattern): buttons, checkboxes, and the settings switch.

use gpui::{
    div, prelude::*, px, rgb, rgba, svg, App, ClickEvent, Context, ElementId, FocusHandle,
    FontWeight, Rems, SharedString, Window,
};

use crate::theme::*;

use super::{focus_ring, h_flex, rem, spinner, tooltip_text};

/// `Primary` = accent fill; `Secondary` = muted fill; `Ghost` = transparent
/// (hover only); `Danger` = red.
pub enum ButtonStyle {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// Shared control scale for buttons and text inputs; `Medium` is the original
/// button size. A given size yields matching dimensions across both — including
/// the icon, which chaining styles after `build` can't reach.
#[derive(Clone, Copy, Default)]
#[allow(dead_code)] // full scale offered; not every step is in use yet
pub enum ControlSize {
    XSmall,
    Small,
    #[default]
    Medium,
    Large,
    XLarge,
}

impl ControlSize {
    /// `(px, py, gap, text, icon)` in zoom-aware rems.
    pub fn metrics(self) -> (Rems, Rems, Rems, Rems, Rems) {
        match self {
            ControlSize::XSmall => (rem(6.0), rem(1.0), rem(3.0), rem(11.0), rem(11.0)),
            ControlSize::Small => (rem(8.0), rem(2.0), rem(4.0), rem(12.0), rem(12.0)),
            ControlSize::Medium => (rem(12.0), rem(4.0), rem(6.0), rem(14.0), rem(14.0)),
            ControlSize::Large => (rem(16.0), rem(6.0), rem(8.0), rem(16.0), rem(16.0)),
            ControlSize::XLarge => (rem(20.0), rem(8.0), rem(10.0), rem(18.0), rem(18.0)),
        }
    }
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct Button {
    id: &'static str,
    label: SharedString,
    style: ButtonStyle,
    size: ControlSize,
    icon: Option<&'static str>,
    disabled: bool,
    loading: bool,
    tooltip: Option<SharedString>,
    focus: Option<FocusHandle>,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: &'static str, label: impl Into<SharedString>, style: ButtonStyle) -> Self {
        Self {
            id,
            label: label.into(),
            style,
            size: ControlSize::default(),
            icon: None,
            disabled: false,
            loading: false,
            tooltip: None,
            focus: None,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    #[allow(dead_code)] // offered alongside `loading`; not every call site disables yet
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Show a spinner in place of the icon and make the button inert (implies
    /// disabled) — for in-flight actions.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Make it keyboard-focusable (Tab order + focus ring).
    pub fn focus(mut self, focus: Option<&FocusHandle>) -> Self {
        self.focus = focus.cloned();
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let inert = self.disabled || self.loading;
        // svg colour doesn't inherit, so match the label colour explicitly.
        let fg = if inert {
            FG_SUBTLE
        } else {
            match self.style {
                ButtonStyle::Primary | ButtonStyle::Danger => 0xffffff,
                _ => FG,
            }
        };
        let (px_h, py_v, gap, text, icon_sz) = self.size.metrics();
        let base = h_flex()
            .id(self.id)
            .flex_shrink_0()
            .gap(gap)
            .items_center()
            .px(px_h)
            .py(py_v)
            .rounded_md()
            .text_size(text)
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(fg))
            .when(self.loading, |b| {
                b.child(spinner(SharedString::from(format!("{}-spin", self.id)), px(13.0), fg))
            })
            .when_some(self.icon.filter(|_| !self.loading), |b, icon| {
                b.child(svg().path(icon).size(icon_sz).flex_shrink_0().text_color(rgb(fg)))
            })
            .child(self.label)
            .map(|b| match self.on_click.filter(|_| !inert) {
                Some(handler) => b.cursor_pointer().on_click(move |ev, window, cx| handler(ev, window, cx)),
                None => b.when(inert, |b| b.cursor_default()),
            });
        // One muted, non-hovering fill for any disabled style; else the style's fill.
        let base = if inert {
            base.bg(rgba(OVERLAY))
        } else {
            match self.style {
                ButtonStyle::Primary => base.bg(rgb(ACCENT)).hover(|s| s.bg(rgb(ACCENT_HOVER))),
                ButtonStyle::Secondary => base.bg(rgba(OVERLAY)).hover(|s| s.bg(rgba(SELECT_MUTED))),
                ButtonStyle::Ghost => base.hover(|s| s.bg(rgba(OVERLAY))),
                ButtonStyle::Danger => base.bg(rgb(DANGER)),
            }
        };
        let base = base.when_some(self.tooltip, |b, t| b.tooltip(tooltip_text(t)));
        match self.focus {
            Some(focus) => focus_ring(base).track_focus(&focus).tab_index(0),
            None => base,
        }
    }
}

/// A checkbox: a box that shows a check when on. Set the handler with `on_click`,
/// like [`Button`].
#[derive(IntoElement)]
pub struct Checkbox {
    id: &'static str,
    label: Option<SharedString>,
    checked: bool,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl Checkbox {
    pub fn new(id: &'static str, checked: bool) -> Self {
        Self { id, label: None, checked, tooltip: None, on_click: None }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let checked = self.checked;
        let box_el = h_flex()
            .size(rem(15.0))
            .flex_shrink_0()
            .justify_center()
            .items_center()
            .rounded_sm()
            .border_1()
            .map(|d| {
                if checked {
                    d.bg(rgb(ACCENT)).border_color(rgb(ACCENT)).child(
                        svg().path("icons/check.svg").size(rem(11.0)).text_color(rgb(0xffff_ffff)),
                    )
                } else {
                    d.border_color(rgb(BORDER_MUTED))
                }
            });
        h_flex()
            .id(self.id)
            .gap_1p5()
            .items_center()
            .cursor_pointer()
            .text_color(rgb(if checked { FG } else { FG_MUTED }))
            .when_some(self.on_click, |el, handler| {
                el.on_click(move |ev, window, cx| handler(ev, window, cx))
            })
            .when_some(self.tooltip, |el, t| el.tooltip(tooltip_text(t)))
            .child(box_el)
            .when_some(self.label, |el, label| el.child(label))
    }
}

/// A switch toggle matching Zed's settings switch: a rounded track (accent when
/// on) with a sliding white thumb (dimmed when off). Set the handler with
/// `on_click`, like [`Button`]; optional keyboard focus.
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    focus: Option<FocusHandle>,
    on: bool,
    on_click: Option<ClickHandler>,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self { id: id.into(), focus: None, on, on_click: None }
    }

    /// Make it keyboard-focusable (Tab order + focus ring).
    pub fn focus(mut self, focus: Option<&FocusHandle>) -> Self {
        self.focus = focus.cloned();
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let on = self.on;
        let track = h_flex()
            .id(self.id)
            .w(rem(28.0))
            .h(rem(16.0))
            .px(px(2.0))
            .flex_shrink_0()
            .items_center()
            .rounded_full()
            .cursor_pointer()
            .bg(rgb(if on { ACCENT } else { INSET }))
            .border_1()
            .border_color(rgb(if on { ACCENT } else { BORDER_MUTED }))
            .when(on, |el| el.justify_end())
            .when_some(self.on_click, |el, handler| el.on_click(move |ev, window, cx| handler(ev, window, cx)))
            // Zed's thumb is the text color, full opacity on / dimmed off.
            .child(div().size(rem(12.0)).rounded_full().bg(rgba(if on { 0xffff_ffff } else { 0xffff_ff80 })));
        match self.focus {
            Some(focus) => track.track_focus(&focus).tab_index(0).focus_visible(|s| s.border_color(rgb(ACCENT))),
            None => track,
        }
    }
}

/// A compact Zed-style switch; focusable (Tab-reachable) when given a handle,
/// toggled by click or Enter/Space (gpui synthesizes the click on key press).
pub fn switch<V: 'static>(
    id: impl Into<ElementId>,
    on: bool,
    focus: Option<&FocusHandle>,
    on_toggle: impl Fn(&mut V, &mut Context<V>) + 'static,
    cx: &mut Context<V>,
) -> Toggle {
    Toggle::new(id, on)
        .focus(focus)
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| on_toggle(this, cx)))
}
