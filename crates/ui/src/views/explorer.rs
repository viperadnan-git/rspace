//! The explorer column: navigation toolbar + breadcrumb around the [`Explorer`]
//! pane entity, with the preview beside it. The listing itself lives in the
//! explorer view.

use super::*;

impl Workspace {
    /// The action bar above the listing: back/forward, directory actions, refresh,
    /// and the search toggle. A fixed height lets the back/forward divider span it
    /// edge to edge.
    fn action_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let fetching = self.explorer().read(cx).is_fetching();
        let search_open = self.explorer().read(cx).search_open();
        h_flex()
            .w_full()
            .flex_shrink_0()
            .h(px(ACTION_BAR_H))
            .gap_2()
            .items_center()
            .justify_between()
            .pl_1()
            .pr_3()
            .border_b_1()
            .border_color(rgb(BORDER_MUTED))
            .child(
                h_flex()
                    .h_full()
                    .gap_1()
                    .items_center()
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
                                this.explorer().update(cx, |e, cx| e.toggle_search(window, cx));
                            })),
                    ),
            )
    }

    /// The pane: the tab strip plus the active tab's body (welcome screen, or the
    /// file-list column with its preview).
    pub(crate) fn render_explorer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .child(self.render_tab_strip(cx))
            .child(self.render_pane_body(cx))
    }

    fn render_pane_body(&self, cx: &mut Context<Self>) -> AnyElement {
        // The welcome screen replaces the body when no remote is open.
        if self.active().open_remote.is_none() {
            return self.render_welcome(cx).into_any_element();
        }
        v_flex()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .overflow_hidden()
            .bg(rgb(INSET))
            .child(self.action_bar(cx))
            .child(self.explorer())
            .when(self.store.get().show_path_bar, |el| el.child(self.render_path_bar(cx)))
            .into_any_element()
    }

    /// The path bar: the [`PathBar`] entity (own width → Finder-style shrinking)
    /// with the copy-path button overlaid at the right edge (kept on the workspace
    /// so its copied-feedback animation still works).
    fn render_path_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .w_full()
            .flex_shrink_0()
            .child(self.path_bar.clone())
            .child(
                div()
                    .absolute()
                    .right(px(4.0))
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .child(self.copy_button("copy-path", CopySource::Path, self.copy_text(), "Copy path", cx)),
            )
    }
}
