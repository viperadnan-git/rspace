//! The explorer column: navigation toolbar + breadcrumb around the [`Explorer`]
//! pane entity, with the preview beside it. The listing itself lives in the
//! explorer view.

use super::*;

impl Workspace {
    /// The crumb row. Deep paths collapse the middle (remote › … › parent ›
    /// current); in tight space ancestor crumbs truncate first while the current
    /// folder stays readable, so it never overflows the column.
    fn render_breadcrumb(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let row = h_flex().flex_1().min_w(px(0.0)).gap_1().items_center().overflow_hidden();
        let Some(remote) = self.open_remote.clone() else {
            return row;
        };

        let mut segs: Vec<(String, String)> = vec![(remote.clone(), String::new())];
        if !self.path.is_empty() {
            let mut acc = String::new();
            for part in self.path.split('/') {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(part);
                segs.push((part.to_string(), acc.clone()));
            }
        }

        let n = segs.len();
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
            let (label, path) = segs[idx].clone();
            let label = if ellipsis { "…".to_string() } else { label };
            let is_last = idx == n - 1;
            let remote = remote.clone();
            let (remote_for_drop, drop_dir) = (remote.clone(), path.clone());
            let crumb = div()
                .id(SharedString::from(format!("crumb-{pos}")))
                .px_1()
                .rounded_md()
                .min_w(px(0.0))
                // The current folder keeps more room; ancestors give way first.
                .max_w(rem(if is_last { 220.0 } else { 120.0 }))
                .truncate()
                .cursor_pointer()
                .text_color(if is_last { rgb(FG) } else { rgb(FG_MUTED) })
                .hover(|s| s.bg(rgba(OVERLAY)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.navigate(remote.clone(), path.clone(), None, cx)
                }))
                .child(label);
            row = row.child(self.entry_drop_target(crumb, remote_for_drop, drop_dir, cx));
        }
        row
    }

    /// The toolbar above the listing: back/forward, directory actions, refresh,
    /// and the search and preview toggles.
    fn explorer_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let fetching = self.explorer.read(cx).is_fetching();
        let search_open = self.explorer.read(cx).search_open();
        h_flex()
            .w_full()
            .py_1p5()
            .gap_2()
            .justify_between()
            .pl_1()
            .pr_3()
            .border_b_1()
            .border_color(rgb(BORDER_MUTED))
            .child(
                h_flex()
                    .h_full()
                    .gap_1()
                    .min_w(px(0.0))
                    .child(nav_button("nav-back", "←", self.can_back()).when(self.can_back(), |b| {
                        b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go_back(cx)))
                    }))
                    .child(nav_button("nav-forward", "→", self.can_forward()).when(
                        self.can_forward(),
                        |b| b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go_forward(cx))),
                    ))
                    .child(div().w(px(1.0)).h_full().mx_1().flex_shrink_0().bg(rgb(BORDER_MUTED)))
                    .child(
                        icon_button("nav-home", "icons/home.svg")
                            .tooltip(tooltip_text("Home"))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go_to_root(cx))),
                    )
                    .child(
                        icon_button("new-folder", "icons/new_folder.svg")
                            .tooltip(tooltip_text("New folder"))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.begin_new_folder(cx))),
                    )
                    .child(
                        icon_button("upload", "icons/upload.svg")
                            .tooltip(tooltip_text("Upload"))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.begin_upload(cx))),
                    )
                    .when(self.clipboard.is_some(), |el| {
                        el.child(
                            icon_button("paste", "icons/clipboard.svg")
                                .tooltip(tooltip_text("Paste"))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.paste_clipboard(cx))),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(FG_MUTED))
                    .when(fetching, |el| el.child(spinner("fetch-spinner", px(12.0), FG_MUTED)))
                    .child(
                        icon_button("refresh", "icons/refresh.svg")
                            .tooltip(tooltip_text(format!(
                                "Refresh ({})",
                                if cfg!(target_os = "macos") { "\u{2318}R" } else { "Ctrl R" }
                            )))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.force_reload_entries(cx))),
                    )
                    .child(
                        icon_button("toggle-search", "icons/search.svg")
                            .when(search_open, |b| b.bg(rgba(SELECT_MUTED)))
                            .tooltip(tooltip_text(format!(
                                "Search ({})",
                                if cfg!(target_os = "macos") { "\u{2318}F" } else { "Ctrl F" }
                            )))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.explorer.update(cx, |e, cx| e.toggle_search(window, cx));
                            })),
                    )
                    .child(
                        icon_button("toggle-preview", "icons/sidebar_right.svg")
                            .when(self.dock_is(DockPanel::Preview), |b| b.bg(rgba(SELECT_MUTED)))
                            .tooltip(tooltip_text("Preview (Space)"))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.toggle_preview(&TogglePreview, window, cx)
                            })),
                    ),
            )
    }

    pub(crate) fn render_explorer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // The welcome screen (no remote open) replaces the whole column, so the
        // preview — a child of this view — can't render without an open remote.
        if self.open_remote.is_none() {
            return self.render_welcome(cx).into_any_element();
        }
        let column = v_flex()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .bg(rgb(INSET))
            .child(self.explorer_toolbar(cx))
            .child(self.explorer.clone())
            .when(self.store.get().show_path_bar, |el| el.child(self.render_path_bar(cx)));
        div()
            .flex()
            .flex_row()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .child(column)
            .when(self.dock_is(DockPanel::Preview), |el| el.child(self.render_preview(cx)))
            .into_any_element()
    }

    /// Finder-style path bar: the breadcrumb in a thin strip below the listing,
    /// toggled by `show_path_bar`. The copy-path button is pinned right, outside
    /// the breadcrumb's shrink/truncate area, so it's always reachable.
    fn render_path_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .px_1()
            .py_0p5()
            .text_xs()
            .border_t_1()
            .border_color(rgb(BORDER_MUTED))
            .child(self.render_breadcrumb(cx))
            .child(self.copy_button("copy-path", CopySource::Path, self.copy_text(), "Copy path", cx))
    }
}
