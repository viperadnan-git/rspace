use super::*;

/// Distance from the list's top/bottom edge (px) within which a marquee drag
/// auto-scrolls, and the per-frame scroll step.
const MARQUEE_EDGE: f32 = 24.0;
const MARQUEE_SCROLL_STEP: f32 = 12.0;

impl Explorer {
    /// Begin (on first call) or continue a rubber-band drag from `anchor` to the
    /// live cursor `current`, both in window coords. `additive` (Cmd/Shift held at
    /// press) keeps the pre-drag selection; otherwise the band replaces it.
    pub(crate) fn drag_marquee(
        &mut self,
        anchor: Point<Pixels>,
        current: Point<Pixels>,
        additive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match &mut self.marquee {
            Some(m) => m.current = current,
            None => {
                if self.entries().is_empty() {
                    return;
                }
                let base = if additive { self.sel.snapshot().clone() } else { HashSet::new() };
                self.marquee = Some(Marquee { anchor, current, base });
                self.start_autoscroll(window, cx);
            }
        }
        self.apply_marquee();
        cx.notify();
    }

    pub(crate) fn end_marquee(&mut self, cx: &mut Context<Self>) {
        if self.marquee.take().is_some() {
            cx.notify();
        }
    }

    /// Recompute the selection from the band's current extent. Rebuilt from scratch
    /// each call (onto the pre-drag `base`), so shrinking the band deselects rows it
    /// no longer covers.
    fn apply_marquee(&mut self) {
        let Some(m) = self.marquee.as_ref() else {
            return;
        };
        let (anchor_y, cur_y) = (m.anchor.y, m.current.y);
        let mut selected = m.base.clone();
        let mut lead = None;
        if let Some((lo, hi)) = self.marquee_rows(anchor_y, cur_y) {
            for ix in lo..=hi {
                if let Some(e) = self.entries().get(ix) {
                    selected.insert(e.path.clone());
                }
            }
            lead = Some(if cur_y >= anchor_y { hi } else { lo });
        }
        self.entry_sel = lead.filter(|_| !selected.is_empty());
        self.sel.set_to(selected);
    }

    /// Edge-scroll loop for a marquee drag; self-terminates once the band ends
    /// (autoscroll_tick returns false), since gpui only fires drag-move on motion.
    fn start_autoscroll(&self, window: &Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(16)).await;
                let alive = cx
                    .update(|_, app| this.update(app, |this, cx| this.autoscroll_tick(cx)))
                    .map(|r| r.unwrap_or(false));
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One auto-scroll frame: nudge the list when the cursor sits in an edge zone,
    /// then re-derive the selection. Returns whether the band is still active.
    fn autoscroll_tick(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(cur_y) = self.marquee.as_ref().map(|m| f32::from(m.current.y)) else {
            return false;
        };
        let st = self.entry_scroll.0.borrow();
        let bounds = st.base_handle.bounds();
        let (top, height) = (f32::from(bounds.top()), f32::from(bounds.size.height));
        let off = st.base_handle.offset();
        let (off_y, max_y) = (f32::from(off.y), f32::from(st.base_handle.max_offset().y));
        let step = if cur_y < top + MARQUEE_EDGE && off_y < 0.0 {
            MARQUEE_SCROLL_STEP
        } else if cur_y > top + height - MARQUEE_EDGE && off_y > -max_y {
            -MARQUEE_SCROLL_STEP
        } else {
            return true;
        };
        let new_y = (off_y + step).clamp(-max_y, 0.0);
        if (new_y - off_y).abs() < 0.5 {
            return true;
        }
        st.base_handle.set_offset(Point { x: off.x, y: px(new_y) });
        drop(st);
        self.apply_marquee();
        cx.notify();
        true
    }

    /// Row indices whose vertical extent intersects the band between window
    /// y-coords `y0` and `y1`. `None` when the list is empty, not yet laid out, or
    /// the band misses the rows entirely.
    fn marquee_rows(&self, y0: Pixels, y1: Pixels) -> Option<(usize, usize)> {
        let st = self.entry_scroll.0.borrow();
        let len = self.entries().len();
        if len == 0 {
            return None;
        }
        // `last_item_size.item` is the viewport, `.contents` the full content stack;
        // a single row is the content height over the row count.
        let row_h = f32::from(st.last_item_size?.contents.height) / len as f32;
        if row_h <= 0.0 {
            return None;
        }
        let top = f32::from(st.base_handle.bounds().top() + st.base_handle.offset().y);
        let bottom = top + row_h * len as f32;
        let (lo, hi) = (f32::from(y0).min(f32::from(y1)), f32::from(y0).max(f32::from(y1)));
        if hi < top || lo > bottom {
            return None;
        }
        let first = ((lo - top) / row_h).floor().max(0.0) as usize;
        let last = (((hi - top) / row_h).floor() as usize).min(len - 1);
        Some((first.min(last), last))
    }

    /// The band rectangle to paint as `(left, top, width, height)` relative to the
    /// list viewport's top-left, clamped to the viewport. `None` when no drag is
    /// active or the list hasn't been laid out.
    pub(crate) fn marquee_rect(&self) -> Option<(Pixels, Pixels, Pixels, Pixels)> {
        let m = self.marquee.as_ref()?;
        let st = self.entry_scroll.0.borrow();
        st.last_item_size?;
        let vp = st.base_handle.bounds();
        let (ox, oy) = (f32::from(vp.left()), f32::from(vp.top()));
        let (w, h) = (f32::from(vp.size.width), f32::from(vp.size.height));
        let cx0 = (f32::from(m.anchor.x) - ox).clamp(0.0, w);
        let cx1 = (f32::from(m.current.x) - ox).clamp(0.0, w);
        let cy0 = (f32::from(m.anchor.y) - oy).clamp(0.0, h);
        let cy1 = (f32::from(m.current.y) - oy).clamp(0.0, h);
        Some((px(cx0.min(cx1)), px(cy0.min(cy1)), px((cx1 - cx0).abs()), px((cy1 - cy0).abs())))
    }
}
