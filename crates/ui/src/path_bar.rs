//! The breadcrumb path bar, as its own entity. Wrapping it in a view gives it a
//! *definite* width — inline elements in the explorer column don't get one, which
//! is why a plain breadcrumb collapsed. With a real width, flex-shrink works, so
//! the crumbs squeeze and truncate to any width like Finder's path bar. It mirrors
//! the active tab's explorer (re-pointed on tab switch, like the preview).

use gpui::{Entity, ScrollHandle, WeakEntity};

use super::*;

pub(crate) struct PathBar {
    workspace: WeakEntity<Workspace>,
    explorer: Entity<Explorer>,
    _obs: gpui::Subscription,
    /// Horizontal scroll for an over-long path (crumbs never shrink, they scroll).
    scroll: ScrollHandle,
    /// Spring-loaded crumbs: a drag dwelling on a path navigates there.
    spring: SpringLoad<String>,
}

impl PathBar {
    pub(crate) fn new(
        workspace: WeakEntity<Workspace>,
        explorer: Entity<Explorer>,
        cx: &mut Context<Self>,
    ) -> Self {
        let obs = cx.observe(&explorer, |_, _, cx| cx.notify());
        Self { workspace, explorer, _obs: obs, scroll: ScrollHandle::new(), spring: SpringLoad::new() }
    }

    /// Re-point at the active tab's explorer (on tab switch).
    pub(crate) fn set_explorer(&mut self, explorer: Entity<Explorer>, cx: &mut Context<Self>) {
        self._obs = cx.observe(&explorer, |_, _, cx| cx.notify());
        self.explorer = explorer;
        cx.notify();
    }

    /// Drag dwelling on a crumb: after a 1s dwell, navigate there so the user can
    /// drop into a different folder along the path (spring-loaded breadcrumb).
    fn spring_hover(&mut self, remote: String, path: String, cx: &mut Context<Self>) {
        // Already here: nothing to spring to.
        if self.explorer.read(cx).location().map(|(_, p)| p).as_deref() == Some(path.as_str()) {
            self.spring.clear();
            return;
        }
        let Some(generation) = self.spring.arm(path.clone()) else { return };
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(SPRING_LOAD_MS)).await;
            this.update(cx, |this, cx| {
                if this.spring.live(generation, &path) {
                    this.workspace
                        .update(cx, |ws, cx| ws.navigate(remote.clone(), path.clone(), None, cx))
                        .ok();
                }
            })
            .ok();
        })
        .detach();
    }

    fn spring_clear(&mut self) {
        self.spring.clear();
    }
}

impl Render for PathBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row = h_flex()
            .id("breadcrumb")
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .px_1()
            // Clear the copy button overlaid by the workspace at the right edge.
            .pr(rem(26.0))
            .py_0p5()
            .text_xs()
            .border_t_1()
            .border_color(rgb(BORDER_MUTED))
            // Crumbs never shrink (no premature truncation); a long path scrolls.
            .overflow_x_scroll()
            .track_scroll(&self.scroll);
        let Some((remote, path)) = self.explorer.read(cx).location() else {
            return row;
        };

        let mut segs: Vec<(String, String)> = vec![(remote.clone(), String::new())];
        if !path.is_empty() {
            let mut acc = String::new();
            for part in path.split('/') {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(part);
                segs.push((part.to_string(), acc.clone()));
            }
        }
        let n = segs.len();
        // Deep paths collapse the middle to "…".
        let visible: Vec<(usize, bool)> = if n <= MAX_CRUMBS {
            (0..n).map(|i| (i, false)).collect()
        } else {
            vec![(0, false), (n - 3, true), (n - 2, false), (n - 1, false)]
        };

        let mut row = row;
        for (pos, (idx, ellipsis)) in visible.into_iter().enumerate() {
            if pos > 0 {
                row = row.child(div().flex_shrink_0().text_color(rgb(FG_SUBTLE)).child("›"));
            }
            if ellipsis {
                row = row.child(div().flex_shrink_0().text_color(rgb(FG_MUTED)).child("…"));
                continue;
            }
            let (label, crumb_path) = segs[idx].clone();
            let is_last = idx == n - 1;
            // The (remote, path) target is shared by all three handlers via a cheap
            // Rc clone, rather than cloning the string pair once per handler.
            let target = std::rc::Rc::new((remote.clone(), crumb_path));
            let crumb = div()
                .id(SharedString::from(format!("crumb-{pos}")))
                .flex_shrink_0()
                .px_1()
                .rounded_md()
                // Each crumb keeps its content width; only an individually huge name
                // truncates (at its max width). The row scrolls if the path is long.
                .max_w(rem(if is_last { 320.0 } else { 200.0 }))
                .truncate()
                .cursor_pointer()
                .text_color(if is_last { rgb(FG) } else { rgb(FG_MUTED) })
                .hover(|s| s.bg(rgba(OVERLAY)))
                .on_click(cx.listener({
                    let target = target.clone();
                    move |this, _: &ClickEvent, _, cx| {
                        let (remote, path) = (target.0.clone(), target.1.clone());
                        this.workspace.update(cx, |ws, cx| ws.navigate(remote, path, None, cx)).ok();
                    }
                }))
                .drag_over::<DraggedEntry>(|s, _, _, _| s.bg(rgba(ACCENT_SOFT)))
                .on_drag_move(cx.listener({
                    let target = target.clone();
                    move |this, e: &DragMoveEvent<DraggedEntry>, _, cx| {
                        if e.bounds.contains(&e.event.position) {
                            this.spring_hover(target.0.clone(), target.1.clone(), cx);
                        } else if this.spring.is_pending(&target.1) {
                            this.spring_clear();
                        }
                    }
                }))
                .on_drop(cx.listener(move |this, d: &DraggedEntry, window, cx| {
                    this.spring_clear();
                    let (d, mods) = (d.clone(), window.modifiers());
                    let (remote, dir) = (target.0.clone(), target.1.clone());
                    this.workspace.update(cx, |ws, cx| ws.drop_into(&d, remote, dir, mods, cx)).ok();
                }))
                .child(label);
            row = row.child(crumb);
        }
        row
    }
}
