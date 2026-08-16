//! Zoomable, pannable image view, reusable wherever an image needs fit-to-pane
//! display. Zoom is fit-relative (1.0 fits the pane); pinch or cmd/ctrl-scroll
//! zooms at the cursor, scroll and drag pan.
//!
//! The image sits in a `pane × zoom` box, centered and shifted by `pan`, drawn by
//! [`ImageContentElement`] — a custom element so it can read the pane bounds that
//! anchoring and clamping need.

use std::sync::Arc;

use gpui::{
    div, img, point, prelude::*, px, relative, size, App, Bounds, Context, CursorStyle,
    DispatchPhase, Element, ElementId, GlobalElementId, Image, InspectorElementId, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, PinchEvent, Pixels, Point,
    Render, ScrollDelta, ScrollWheelEvent, Style, Window,
};

use super::*;

/// Zoom bounds (1.0 = fit-to-pane, the floor).
const ZOOM_MIN: f32 = 1.0;
const ZOOM_MAX: f32 = 8.0;
/// Wheel lines → pixels, for mice reporting line deltas.
const LINE_PX: f32 = 16.0;

pub(crate) struct ImageView {
    image: Option<Arc<Image>>,
    zoom: f32,
    /// Image shift from centered, in pixels; clamped so the box can't leave the pane.
    pan: Point<Pixels>,
    /// Cursor at the last drag sample while a button is held; `None` when idle.
    drag_from: Option<Point<Pixels>>,
    /// Last painted pane bounds — the frame of reference for anchoring and clamping.
    bounds: Option<Bounds<Pixels>>,
}

impl ImageView {
    pub(crate) fn new() -> Self {
        Self { image: None, zoom: ZOOM_MIN, pan: Point::default(), drag_from: None, bounds: None }
    }

    /// Show `image`, resetting the view when it's a different image (the same
    /// `Arc` re-shown keeps zoom/pan — e.g. a cached re-selection).
    pub(crate) fn show(&mut self, image: Arc<Image>, cx: &mut Context<Self>) {
        if self.image.as_ref().is_some_and(|c| Arc::ptr_eq(c, &image)) {
            return;
        }
        self.image = Some(image);
        self.zoom = ZOOM_MIN;
        self.pan = Point::default();
        cx.notify();
    }

    fn is_zoomed(&self) -> bool {
        self.zoom > ZOOM_MIN
    }

    /// Set zoom, keeping the point under `anchor` (window coords) fixed on screen.
    fn set_zoom(&mut self, zoom: f32, anchor: Option<Point<Pixels>>, cx: &mut Context<Self>) {
        let old = self.zoom;
        self.zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        if let Some((anchor, bounds)) = anchor.zip(self.bounds) {
            let from_center = point(
                anchor.x - bounds.origin.x - bounds.size.width / 2.0,
                anchor.y - bounds.origin.y - bounds.size.height / 2.0,
            );
            let ratio = self.zoom / old;
            self.pan += (from_center - self.pan) * (1.0 - ratio);
        }
        self.clamp_pan();
        cx.notify();
    }

    /// Keep the (centered) `pane × zoom` box covering the pane: no background gap.
    fn clamp_pan(&mut self) {
        let Some(bounds) = self.bounds else { return };
        let slack_x = (bounds.size.width * self.zoom - bounds.size.width).max(px(0.0)) / 2.0;
        let slack_y = (bounds.size.height * self.zoom - bounds.size.height).max(px(0.0)) / 2.0;
        self.pan.x = self.pan.x.clamp(-slack_x, slack_x);
        self.pan.y = self.pan.y.clamp(-slack_y, slack_y);
    }

    fn on_scroll(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if e.modifiers.control || e.modifiers.platform {
            let dy = match e.delta {
                ScrollDelta::Pixels(p) => f32::from(p.y),
                ScrollDelta::Lines(p) => p.y * LINE_PX,
            };
            self.set_zoom(self.zoom * (1.0 + dy * 0.004), Some(e.position), cx);
        } else {
            let delta = match e.delta {
                ScrollDelta::Pixels(p) => p,
                ScrollDelta::Lines(p) => p.map(|d| px(d * LINE_PX)),
            };
            self.pan += delta;
            self.clamp_pan();
            cx.notify();
        }
    }

    fn on_pinch(&mut self, e: &PinchEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom * (1.0 + e.delta), Some(e.position), cx);
    }

    fn on_mouse_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.drag_from = Some(e.position);
        cx.notify();
    }

    fn on_mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(from) = self.drag_from else { return };
        self.pan += e.position - from;
        self.drag_from = Some(e.position);
        self.clamp_pan();
        cx.notify();
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag_from.take().is_some() {
            cx.notify();
        }
    }
}

impl Render for ImageView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.image.is_none() {
            return centered("No image", FG_SUBTLE).into_any_element();
        }
        let cursor = match (self.is_zoomed(), self.drag_from.is_some()) {
            (false, _) => CursorStyle::Arrow,
            (true, false) => CursorStyle::OpenHand,
            (true, true) => CursorStyle::ClosedHand,
        };
        div()
            .id("image-view")
            .size_full()
            .overflow_hidden()
            .cursor(cursor)
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_pinch(cx.listener(Self::on_pinch))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _: &MouseUpEvent, _, cx| this.end_drag(cx)))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(ImageContentElement { view: cx.entity() })
            .into_any_element()
    }
}

/// Paints the image at `pane × zoom`, centered and panned. A custom element so
/// it can capture the pane bounds that [`ImageView`] anchors and clamps against.
struct ImageContentElement {
    view: gpui::Entity<ImageView>,
}

impl IntoElement for ImageContentElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ImageContentElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<gpui::AnyElement>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style { size: size(relative(1.).into(), relative(1.).into()), ..Default::default() };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let (image, zoom, pan) = self.view.update(cx, |v, _| {
            v.bounds = Some(bounds);
            v.clamp_pan();
            (v.image.clone(), v.zoom, v.pan)
        });
        let image = image?;
        let box_w = bounds.size.width * zoom;
        let box_h = bounds.size.height * zoom;
        let left = bounds.size.width / 2.0 - box_w / 2.0 + pan.x;
        let top = bounds.size.height / 2.0 - box_h / 2.0 + pan.y;

        let mut element = div()
            .relative()
            .size_full()
            .child(
                div().absolute().left(left).top(top).w(box_w).h(box_h).child(
                    img(image)
                        .id("image-view-img")
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .with_fallback(|| centered("Can't preview this image", FG_SUBTLE).into_any_element()),
                ),
            )
            .into_any_element();
        element.prepaint_as_root(bounds.origin, bounds.size.into(), window, cx);
        Some(element)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // End a drag even if the button is released outside the pane.
        if self.view.read(cx).drag_from.is_some() {
            let view = self.view.downgrade();
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble
                    && let Some(view) = view.upgrade()
                {
                    view.update(cx, |v, cx| v.end_drag(cx));
                }
            });
        }
        if let Some(element) = prepaint {
            element.paint(window, cx);
        }
    }
}
