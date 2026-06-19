//! The file explorer: breadcrumb, column header, search bar, entry list.

use super::*;

impl Workspace {
    fn render_error(&self, message: String, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full().justify_center().items_center().p_8().child(
            v_flex()
                .max_w(px(440.0))
                .items_center()
                .gap_3()
                .child(svg().path("icons/alert.svg").size(px(28.0)).text_color(rgb(DANGER)))
                .child(div().text_color(rgb(FG)).child("Failed to load"))
                .child(
                    div()
                        .w_full()
                        .max_h(px(180.0))
                        .overflow_hidden()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(INSET))
                        .border_1()
                        .border_color(rgb(BORDER_MUTED))
                        .text_xs()
                        .text_color(rgb(FG_MUTED))
                        .child(message.clone()),
                )
                .child(
                    h_flex().w_full().justify_end().child(self.copy_button(
                        "copy-error",
                        CopySource::Error,
                        message,
                        "Copy error",
                        cx,
                    )),
                ),
        )
    }

    // Deep paths collapse the middle: remote › … › parent › current.
    fn render_breadcrumb(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let container = h_flex().gap_1().min_w(px(0.0));
        let Some(remote) = self.open_remote.clone() else {
            return container.child(div().text_color(rgb(FG_SUBTLE)).child("Select a remote"));
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

        let mut row = container;
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
                .flex_shrink_0()
                .max_w(px(160.0))
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
        row.child(self.copy_button("copy-path", CopySource::Path, self.copy_text(), "Copy path", cx).ml_1())
    }

    // deferred: paints over layout without consuming space
    fn col_head(
        &self,
        field: SortField,
        label: &'static str,
        width: Option<Pixels>,
        right: bool,
        resize: Option<Column>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let active = self.sort_field == field;
        let base = h_flex()
            .id(label)
            .gap_1()
            .cursor_pointer()
            .text_color(rgb(FG_SUBTLE))
            .hover(|s| s.text_color(rgb(FG_MUTED)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.choose_sort(field, cx)))
            .when(right, |x| x.justify_end())
            .child(label)
            .when(active, |x| x.child(div().text_color(rgb(FG_MUTED)).child(sort_arrow(self.sort_order))))
            .when_some(resize, |x, col| x.relative().child(self.column_resize_handle(col, cx)));
        match width {
            Some(w) => base.px_2().w(w).flex_shrink_0().overflow_hidden(),
            None => base.pr_2().flex_1().min_w(px(0.0)),
        }
    }

    /// A draggable divider on a column's left edge; double-click resets its width.
    fn column_resize_handle(&self, col: Column, cx: &mut Context<Self>) -> impl IntoElement {
        let id: &'static str = match col {
            Column::Date => "col-resize-date",
            Column::Size => "col-resize-size",
        };
        deferred(
            h_flex()
                .id(id)
                .absolute()
                .top(px(0.0))
                .left(px(-RESIZE_HANDLE_W / 2.0))
                .w(px(RESIZE_HANDLE_W))
                .h_full()
                .justify_center()
                .cursor_col_resize()
                .occlude()
                .on_drag(DragColumn(col), move |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| DragColumn(col))
                })
                .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                    if e.click_count() >= 2 {
                        this.reset_column(col, cx);
                    }
                }))
                .child(div().w(px(1.0)).h(px(13.0)).bg(rgb(BORDER_MUTED))),
        )
    }

    fn column_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .flex_shrink_0()
            .px_3()
            .py_1()
            .text_xs()
            .text_color(rgb(FG_SUBTLE))
            .border_b_1()
            .border_color(rgb(BORDER_MUTED))
            .child(self.col_head(SortField::Name, "Name", None, false, None, cx))
            .child(self.col_head(SortField::Size, "Size", Some(self.col_size_width), true, Some(Column::Size), cx))
            .child(self.col_head(
                SortField::Modified,
                "Date Modified",
                Some(self.col_date_width),
                false,
                Some(Column::Date),
                cx,
            ))
    }

    fn search_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .key_context("ExplorerSearch")
            .on_action(cx.listener(Self::search_submit))
            .on_action(cx.listener(Self::close_search))
            .w_full()
            .h(px(34.0))
            .flex_shrink_0()
            .gap_2()
            .px_3()
            .items_center()
            .border_b_1()
            .border_color(rgb(BORDER_MUTED))
            .child(
                svg()
                    .path("icons/search.svg")
                    .size(px(14.0))
                    .flex_shrink_0()
                    .text_color(rgb(FG_SUBTLE)),
            )
            .child(div().flex_grow(1.0).min_w(px(0.0)).child(self.search_input.clone()))
            .when(!self.search.is_empty(), |el| {
                let active = self.recursive_intent();
                el.child(
                    icon_button("search-clear", "icons/x.svg")
                        .tooltip(tooltip_text("Clear"))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.clear_search(cx))),
                )
                .child(
                    Button::new(
                        "search-subfolders",
                        "Subfolders",
                        if active { ButtonStyle::Primary } else { ButtonStyle::Soft },
                    )
                        .icon("icons/corner_down_left.svg")
                        .height(px(24.0))
                        .build(|this, cx| this.toggle_subfolder_search(cx), cx)
                        .text_xs()
                        .tooltip(tooltip_text("Search all subfolders (Enter)")),
                )
            })
    }

    pub(crate) fn render_explorer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.entries().len();
        let making_new = self.prompt.as_ref().is_some_and(|p| p.read(cx).target.is_none());
        let search_error = self
            .recursive_intent()
            .then(|| match self.search_query.status() {
                Status::Error(m) => Some(m.clone()),
                _ => None,
            })
            .flatten();
        let body = if self.open_remote.is_none() {
            self.render_welcome(cx).into_any_element()
        } else if matches!(self.dir_query.status(), Status::Loading) {
            loading_view().into_any_element()
        } else if let Status::Error(message) = self.dir_query.status() {
            self.render_error(message.clone(), cx).into_any_element()
        } else if self.recursive_intent() && matches!(self.search_query.status(), Status::Loading) {
            loading_view().into_any_element()
        } else if let Some(message) = search_error {
            self.render_error(message, cx).into_any_element()
        } else if count == 0 && !making_new {
            let msg = if self.search.is_empty() { "This folder is empty" } else { "No matches" };
            centered(msg, FG_SUBTLE).into_any_element()
        } else {
            uniform_list(
                "entries",
                count,
                cx.processor(|this, range: Range<usize>, _window, cx| {
                    let focused = this.pane == Pane::Explorer;
                    let matcher = Matcher::new(&this.search);
                    range
                        .filter_map(|ix| this.entries().get(ix).map(|e| (ix, e.clone())))
                        .map(|(ix, entry)| {
                            let renaming = this.prompt.as_ref().is_some_and(|p| {
                                p.read(cx).target.as_deref() == Some(entry.path.as_str())
                            });
                            if renaming {
                                return this.prompt.as_ref().unwrap().clone().into_any_element();
                            }
                            let selected = this.selected.contains(&entry.path);
                            let is_dir = entry.is_dir;
                            let size_label = if is_dir { "--".to_string() } else { human_size(entry.size) };
                            let date_label = human_date(&entry.mod_time);
                            let name = entry.name.clone();
                            // Recursive results show the relative path so the match's
                            // location is visible; the current dir shows just the name.
                            let label = if this.recursive_showing() { entry.path.clone() } else { name.clone() };
                            let ctx_entry = entry.clone();
                            let drag = DraggedEntry {
                                path: entry.path.clone(),
                                name: name.clone(),
                                is_dir,
                                count: if selected { this.selected.len().max(1) } else { 1 },
                            };
                            let drop_path = entry.path.clone();
                            list_item(ix, selected, focused)
                                .h(px(ROW_H))
                                .py(px(0.0))
                                .gap_0()
                                .border_b_1()
                                .border_color(rgb(SEPARATOR))
                                .on_drag(drag, |d, _, _, app| {
                                    let text: SharedString = if d.count > 1 {
                                        format!("{} items", d.count).into()
                                    } else {
                                        d.name.clone().into()
                                    };
                                    app.new(|_| DragLabel { text })
                                })
                                .when(is_dir, |r| {
                                    let dst = this.open_remote.clone().unwrap_or_default();
                                    this.entry_drop_target(r, dst, drop_path, cx)
                                })
                                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                                    this.pane = Pane::Explorer;
                                    this.context = None;
                                    this.prompt = None;
                                    this.focus.focus(window, cx);
                                    if ev.click_count() >= 2 {
                                        this.select_only(ix);
                                        this.descend(ix, cx);
                                        return;
                                    }
                                    let m = ev.modifiers();
                                    if m.secondary() {
                                        this.toggle_at(ix);
                                    } else if m.shift {
                                        this.select_range_to(ix);
                                    } else {
                                        this.select_only(ix);
                                    }
                                    cx.notify();
                                }))
                                // Keep a row's left-press from reaching the body's
                                // deselect handler (the row's on_click does the selecting).
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|_, _: &MouseDownEvent, _, cx| cx.stop_propagation()),
                                )
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.pane = Pane::Explorer;
                                        this.bg_menu = None;
                                        if !this.selected.contains(&ctx_entry.path) {
                                            this.select_only(ix);
                                        }
                                        this.context = Some((ctx_entry.clone(), ev.position));
                                        cx.notify();
                                    }),
                                )
                                .child(
                                    h_flex()
                                        .id(SharedString::from(format!("name-{ix}")))
                                        .gap_2()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .pr_2()
                                        .tooltip(tooltip_text(label.clone()))
                                        .when(is_dir, |r| r.child(file_icon(true)))
                                        .child(if matcher.is_empty() {
                                            div().flex_1().min_w(px(0.0)).truncate().child(label).into_any_element()
                                        } else {
                                            highlighted_label(&label, &matcher.positions(&label), FG, ACCENT)
                                                .into_any_element()
                                        }),
                                )
                                .child(
                                    h_flex()
                                        .w(this.col_size_width)
                                        .flex_shrink_0()
                                        .px_2()
                                        .justify_end()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_xs()
                                        .text_color(rgb(FG_MUTED))
                                        .child(size_label),
                                )
                                .child(
                                    div()
                                        .w(this.col_date_width)
                                        .flex_shrink_0()
                                        .px_2()
                                        .truncate()
                                        .text_xs()
                                        .text_color(rgb(FG_MUTED))
                                        .child(date_label),
                                )
                                .into_any_element()
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.entry_scroll)
            .flex_1()
            .into_any_element()
        };

        let new_item = making_new && self.open_remote.is_some();
        let show_table = self.open_remote.is_some()
            && !matches!(self.dir_query.status(), Status::Loading | Status::Error(_));
        let body_area = v_flex()
            .flex_1()
            .min_h(px(0.0))
            .min_w(px(0.0))
            .overflow_hidden()
            .on_drag_move(cx.listener(Self::on_column_drag))
            .when(show_table && self.search_open, |el| el.child(self.search_bar(cx)))
            .when(show_table, |el| el.child(self.column_header(cx)))
            .when(new_item, |el| el.child(self.prompt.as_ref().unwrap().clone()))
            .child(v_flex().flex_1().min_h(px(0.0)).min_w(px(0.0)).child(body))
            .when(
            self.open_remote.is_some(),
            |el| {
                el.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                        this.pane = Pane::Explorer;
                        this.context = None;
                        this.bg_menu = Some(ev.position);
                        cx.notify();
                    }),
                )
                // Click in empty space deselects (Finder-style). Rows stop
                // propagation, so this only fires off the rows.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, window, cx| {
                        this.pane = Pane::Explorer;
                        this.context = None;
                        this.bg_menu = None;
                        this.focus.focus(window, cx);
                        if !this.selected.is_empty() {
                            this.selected.clear();
                            cx.notify();
                        }
                    }),
                )
                // Files dragged from Finder drop into the open directory; the list
                // tints accent while they hover (bg only, so rows stay flush to
                // the edge and nothing shifts).
                .drag_over::<ExternalPaths>(|s, _, _, _| s.bg(rgba(ACCENT_SOFT)))
                .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                    this.upload_paths(paths.paths().to_vec(), cx);
                }))
            },
        );

        let content = v_flex()
            .flex_1()
            .min_w(px(0.0))
            .bg(rgb(INSET))
            .children(self.open_remote.is_some().then(|| {
                h_flex()
                    .w_full()
                    .h(px(34.0))
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
                            .child(nav_button("nav-back", "←", self.can_back()).when(
                                self.can_back(),
                                |b| b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go_back(cx))),
                            ))
                            .child(nav_button("nav-forward", "→", self.can_forward()).when(
                                self.can_forward(),
                                |b| b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go_forward(cx))),
                            ))
                            .child(div().w(px(1.0)).h_full().mx_1().flex_shrink_0().bg(rgb(BORDER_MUTED)))
                            .child(self.render_breadcrumb(cx)),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(rgb(FG_MUTED))
                            .when(self.dir_query.is_fetching(), |el| {
                                el.child(spinner("fetch-spinner", px(12.0), FG_MUTED))
                            })
                            .child(
                                icon_button("refresh", "icons/refresh.svg")
                                    .tooltip(tooltip_text(format!(
                                        "Refresh ({})",
                                        if cfg!(target_os = "macos") { "\u{2318}R" } else { "Ctrl R" }
                                    )))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.force_reload_entries(cx)
                                    })),
                            )
                            .child(
                                icon_button("toggle-search", "icons/search.svg")
                                    .when(self.search_open, |b| b.bg(rgba(SELECT_MUTED)))
                                    .tooltip(tooltip_text(format!(
                                        "Search ({})",
                                        if cfg!(target_os = "macos") { "\u{2318}F" } else { "Ctrl F" }
                                    )))
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.toggle_search(window, cx)
                                    })),
                            )
                            .child(
                                icon_button("toggle-preview", "icons/sidebar_right.svg")
                                    .when(self.preview_open, |b| b.bg(rgba(SELECT_MUTED)))
                                    .tooltip(tooltip_text("Preview (Space)"))
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.toggle_preview(&TogglePreview, window, cx)
                                    })),
                            ),
                    )
                }))
            .child(body_area);

        // The preview pane belongs to the file-list view, so it is built only in
        // the open-remote branch — the layout structure (not a scattered guard)
        // keeps it off the welcome screen.
        if self.open_remote.is_none() {
            content.into_any_element()
        } else {
            // Plain flex_row, not h_flex: the panes stretch to full height
            // (h_flex's items_center would collapse them to content height).
            div()
                .flex()
                .flex_row()
                .flex_1()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .child(content)
                .when(self.preview_open, |el| el.child(self.render_preview(cx)))
                .into_any_element()
        }
    }

}
