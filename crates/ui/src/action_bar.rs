//! The action bar: one entity owning the whole strip above the listing —
//! back/forward, the locator (breadcrumb that morphs into search), and the
//! actions (refresh, search toggle, `+` menu). It's a thin view over the active
//! `(Explorer, Workspace)`: navigation and file ops live on the workspace, search
//! data lives per-tab on the explorer; this only renders them and owns the bar's
//! own interactions (search toggle/focus, spring-loaded crumbs). One per pane,
//! bound to that pane's explorer for life.

use gpui::{svg, Entity, WeakEntity};

use crate::explorer::{CloseSearch, SearchSubmit};

use super::*;

pub(crate) struct ActionBar {
    workspace: WeakEntity<Workspace>,
    explorer: Entity<Explorer>,
    _obs: gpui::Subscription,
    /// Spring-loaded crumbs: a drag dwelling on a path navigates there.
    spring: SpringLoad<String>,
    /// One-shot: focus the search input on the first render after search opens.
    /// The input lives on the explorer, so it can only be focused once it's in the
    /// tree — i.e. from this view's render (cf. PromptModal).
    focus_search: bool,
    /// Horizontal scroll of the breadcrumb (Zed-style): a long path scrolls rather
    /// than collapsing to an ellipsis; persisted across frames.
    crumb_scroll: ScrollHandle,
    /// The location the breadcrumb last rendered, so a navigation scrolls the new
    /// path's current folder into view once (manual scrolling untouched otherwise).
    crumb_at: Option<String>,
}

impl ActionBar {
    pub(crate) fn new(
        workspace: WeakEntity<Workspace>,
        explorer: Entity<Explorer>,
        cx: &mut Context<Self>,
    ) -> Self {
        let obs = cx.observe(&explorer, |_, _, cx| cx.notify());
        Self {
            workspace,
            explorer,
            _obs: obs,
            spring: SpringLoad::new(),
            focus_search: false,
            crumb_scroll: ScrollHandle::new(),
            crumb_at: None,
        }
    }

    /// Toggle the search field; opening arms the one-shot focus so the input takes
    /// the keyboard on the next render. Driven by the search button and ⌘F.
    pub(crate) fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let opened = self.explorer.update(cx, |e, cx| e.toggle_search(window, cx));
        self.focus_search = opened;
        cx.notify();
    }

    /// Drag dwelling on a crumb: after a dwell, navigate there so the user can drop
    /// into a different folder along the path (spring-loaded breadcrumb).
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

    /// The search field shown while searching: leading glyph, the shared input,
    /// and (once non-empty) a recursive-search toggle. The `ExplorerSearch` key
    /// context routes Enter/Escape to the explorer.
    fn search_field(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let input = self.explorer.read(cx).search_input();
        let empty = self.explorer.read(cx).search_is_empty();
        let active = self.explorer.read(cx).recursive_intent();
        h_flex()
            .key_context("ExplorerSearch")
            .on_action(cx.listener(|this, a: &SearchSubmit, window, cx| {
                this.explorer.update(cx, |e, cx| e.search_submit(a, window, cx));
            }))
            .on_action(cx.listener(|this, a: &CloseSearch, window, cx| {
                this.explorer.update(cx, |e, cx| e.close_search(a, window, cx));
            }))
            .w_full()
            .h_full()
            .pl_1()
            .gap_2()
            .items_center()
            .child(svg().path("icons/search.svg").size(rem(14.0)).flex_shrink_0().text_color(rgb(FG_SUBTLE)))
            .child(div().flex_grow(1.0).min_w(px(0.0)).child(input))
            .when(!empty, |el| {
                el.child(
                    Button::new(
                        "search-subfolders",
                        "Subfolders",
                        if active { ButtonStyle::Primary } else { ButtonStyle::Soft },
                    )
                    .icon("icons/corner_down_left.svg")
                    .size(ControlSize::Small)
                    .tooltip("Search all subfolders (Enter)")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.explorer.update(cx, |e, cx| e.toggle_subfolder_search(cx))
                    })),
                )
            })
    }

    /// The breadcrumb: clickable, drop-targetable crumbs for the open location.
    /// Finder-style — every crumb stays visible and the row fits the bar width;
    /// when space runs short, crumb *names* truncate (`Long Folder…`) rather than
    /// the path collapsing to a single `…`. Non-current crumbs give up width first,
    /// so the current folder and the separators stay legible longest.
    fn breadcrumb(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let row = h_flex()
            .id("breadcrumb")
            .w_full()
            .h_full()
            .items_center()
            .gap_1()
            .text_xs()
            // Zed-style: a long path scrolls horizontally instead of collapsing.
            .overflow_x_scroll()
            .track_scroll(&self.crumb_scroll);

        let loc = self.explorer.read(cx).location();
        let key = loc.as_ref().map(|(r, p)| format!("{r}\u{0}{p}"));
        let navigated = key != self.crumb_at;
        if navigated {
            self.crumb_at = key;
        }

        let Some((remote, path)) = loc else {
            return row.into_any_element();
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
        // On navigation, scroll the current folder (the last crumb) into view. gpui
        // applies this at prepaint with measured bounds — no flicker — and it's a
        // one-shot, so manual scrolling is untouched afterwards. The row interleaves
        // a `›` separator before every crumb past the first, so crumb `p` is child
        // `2p`; the last crumb is child `2(n-1)`.
        if navigated {
            self.crumb_scroll.scroll_to_item(2 * (n - 1));
        }

        let mut row = row;
        for (pos, (label, crumb_path)) in segs.into_iter().enumerate() {
            if pos > 0 {
                row = row.child(div().flex_shrink_0().text_color(rgb(FG_SUBTLE)).child("›"));
            }
            let is_last = pos == n - 1;
            // The (remote, path) target is shared by all three handlers via a cheap
            // Rc clone, rather than cloning the string pair once per handler.
            let target = std::rc::Rc::new((remote.clone(), crumb_path));
            let crumb = div()
                .id(SharedString::from(format!("crumb-{pos}")))
                // Never shrinks (the row scrolls for depth), but a single very long
                // name caps with an ellipsis — careful truncation that can't collapse
                // the whole path.
                .flex_shrink_0()
                .max_w(rem(180.0))
                .truncate()
                .px_1()
                .rounded_md()
                .cursor_pointer()
                .text_color(if is_last { rgb(FG) } else { rgb(FG_MUTED) })
                .hover(|s| s.bg(rgba(OVERLAY)))
                // Full crumb name (not the path), for when it's truncated.
                .tooltip(tooltip_text(label.clone()))
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
        row.into_any_element()
    }
}

impl Render for ActionBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fetching = self.explorer.read(cx).is_fetching();
        let search_open = self.explorer.read(cx).search_open();
        if search_open && self.focus_search {
            self.focus_search = false;
            self.explorer.read(cx).search_input().focus_handle(cx).focus(window, cx);
        }
        let (can_back, can_forward) = self
            .workspace
            .upgrade()
            .map(|ws| {
                let w = ws.read(cx);
                (w.can_back(), w.can_forward())
            })
            .unwrap_or((false, false));
        let mod_key = if cfg!(target_os = "macos") { "\u{2318}" } else { "Ctrl " };

        let locator = if search_open {
            self.search_field(cx).into_any_element()
        } else {
            self.breadcrumb(cx)
        };

        h_flex()
            .w_full()
            .flex_shrink_0()
            .h(px(ACTION_BAR_H))
            .gap_1()
            .items_center()
            .pl_1()
            .pr_1()
            .border_b_1()
            .border_color(rgb(BORDER_MUTED))
            .child(nav_button("nav-back", "←", can_back).when(can_back, |b| {
                b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.workspace.update(cx, |ws, cx| ws.go_back(cx)).ok();
                }))
            }))
            .child(nav_button("nav-forward", "→", can_forward).when(can_forward, |b| {
                b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.workspace.update(cx, |ws, cx| ws.go_forward(cx)).ok();
                }))
            }))
            .child(v_divider())
            // Locator: breadcrumb ⇄ search, owns all remaining width.
            .child(div().flex_1().min_w(px(0.0)).h_full().child(locator))
            // Refresh hides while searching to keep the field roomy.
            .when(fetching, |el| el.child(spinner("fetch-spinner", px(12.0), FG_MUTED)))
            .when(!search_open, |el| {
                el.child(
                    icon_button("refresh", "icons/refresh.svg")
                        .tooltip(tooltip_text(format!("Refresh ({mod_key}R)")))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.explorer.update(cx, |e, cx| e.force_reload_entries(cx));
                        })),
                )
            })
            // While searching, this becomes a clear button: closing search resets
            // the query, so one click both clears and closes.
            .child(
                icon_button(
                    "toggle-search",
                    if search_open { "icons/x.svg" } else { "icons/search.svg" },
                )
                .when(search_open, |b| b.bg(rgba(SELECT_MUTED)))
                .tooltip(tooltip_text(if search_open {
                    "Clear search (Esc)".to_string()
                } else {
                    format!("Search ({mod_key}F)")
                }))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.toggle_search(window, cx))),
            )
            // Directory actions: pinned far right, always visible.
            .child(
                icon_button("dir-actions", "icons/plus.svg")
                    .tooltip(tooltip_text("New folder, upload, paste…"))
                    .on_click(cx.listener(|this, e: &ClickEvent, _, cx| {
                        let pos = e.position();
                        this.workspace.update(cx, |ws, cx| ws.open_actions_menu(pos, cx)).ok();
                    })),
            )
    }
}
