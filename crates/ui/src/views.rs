//! Explorer, sidebar, breadcrumb, chrome, and dialog views.

use super::*;

impl Workspace {
    /// Make `el` a drop target that moves (or copies, with Option held) the
    /// dragged entries into `dst_remote:dst_dir`.
    fn entry_drop_target(
        &self,
        el: Stateful<Div>,
        dst_remote: String,
        dst_dir: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        el.drag_over::<DraggedEntry>(|s, _, _, _| s.bg(rgba(SELECT))).on_drop(cx.listener(
            move |this, d: &DraggedEntry, window, cx| {
                this.drop_into(d, dst_remote.clone(), dst_dir.clone(), window.modifiers().alt, cx)
            },
        ))
    }

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
            // Dropping entries on a crumb moves them into that ancestor directory.
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

    // Overlay on the sidebar's right border; takes no layout space, so
    // `deferred` paints/hit-tests it over the next pane.
    /// A draggable resize strip on a pane edge (the preview's left, the sidebar's
    /// right); double-click resets to `default`.
    pub(crate) fn resize_handle(
        &self,
        id: &'static str,
        target: ResizeTarget,
        default: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let edge = px(-RESIZE_HANDLE_W / 2.0);
        let left_edge = matches!(target, ResizeTarget::Preview);
        deferred(
            div()
                .id(id)
                .absolute()
                .top(px(0.0))
                .when(left_edge, |d| d.left(edge))
                .when(!left_edge, |d| d.right(edge))
                .w(px(RESIZE_HANDLE_W))
                .h_full()
                .cursor_col_resize()
                .occlude()
                .on_drag(DragResize(target), move |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| DragResize(target))
                })
                .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                    if e.click_count() >= 2 {
                        match target {
                            ResizeTarget::Sidebar => this.sidebar_width = px(default),
                            ResizeTarget::Preview => this.preview_width = px(default),
                        }
                        cx.notify();
                    }
                })),
        )
    }

    fn remote_row(
        &self,
        ix: usize,
        remote: RemoteInfo,
        pinned: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let focused = self.pane == Pane::Sidebar;
        let selected = ix == self.remote_sel;
        let menu_name = remote.name.clone();
        let mut row = list_item(ix, selected, focused)
            .tooltip(tooltip_text(format!("{} · {}", remote.name, remote.kind)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.load_remote(ix, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    this.remote_menu = Some((menu_name.clone(), ev.position));
                    cx.notify();
                }),
            )
            .child(
                svg()
                    .path(remote_icon(&remote.kind))
                    .size(px(15.0))
                    .flex_shrink_0()
                    .text_color(rgb(FG_MUTED)),
            )
            .child(
                div()
                    .flex_grow(1.0)
                    .min_w(px(0.0))
                    .truncate()
                    .text_color(rgb(FG))
                    .child(remote.name.clone()),
            )
            .when(pinned, |r| {
                r.child(svg().path("icons/pin.svg").size(px(11.0)).flex_shrink_0().text_color(rgb(FG_SUBTLE)))
            });

        // Dropping entries on a remote moves them into that remote's root.
        row = self.entry_drop_target(row, remote.name.clone(), String::new(), cx);

        if pinned {
            let drag_name = remote.name.clone();
            let target = remote.name.clone();
            row = row
                .on_drag(DraggedRemote { name: drag_name }, |d, _, _, app| {
                    app.new(|_| DragLabel { text: d.name.clone().into() })
                })
                .drag_over::<DraggedRemote>(|s, _, _, _| s.bg(rgba(SELECT_MUTED)))
                .on_drop(cx.listener(move |this, d: &DraggedRemote, _, cx| {
                    this.reorder_pinned(&d.name, &target, cx)
                }));
        }
        row
    }

    pub(crate) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.remotes.len();
        v_flex()
            .relative()
            .w(self.sidebar_width)
            .flex_shrink_0()
            .overflow_hidden()
            .bg(rgb(INSET))
            .border_r_1()
            .border_color(rgb(BORDER_MUTED))
            .child(self.resize_handle("sidebar-resize", ResizeTarget::Sidebar, SIDEBAR_W, cx))
            .child(
                h_flex()
                    .w_full()
                    .px_3()
                    .py_2()
                    .justify_between()
                    .items_center()
                    .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child("REMOTES"))
                    .child(
                        icon_button("add-remote", "icons/plus.svg")
                            .tooltip(tooltip_text("Add remote"))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.begin_add_remote(cx))),
                    ),
            )
            .child(
                // Single list so pinned rows (which lead it) scroll with the rest.
                uniform_list(
                    "remotes",
                    count,
                    cx.processor(|this, range: Range<usize>, _window, cx| {
                        let ordered = this.ordered_remotes();
                        let pinned_count = this.pinned_remotes().len();
                        range
                            .filter_map(|ix| ordered.get(ix).map(|r| (ix, r.clone())))
                            .map(|(ix, remote)| this.remote_row(ix, remote, ix < pinned_count, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.remote_scroll)
                .flex_1(),
            )
    }

    /// Inline text field for new-folder / rename: a bare [`TextInput`] in a
    /// row styled to sit in the file list.
    /// A clickable file-list column header (toggles/sets the sort like Finder).
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
            // Inset content so it doesn't sit on the resize divider (Finder-style).
            Some(w) => base.px_2().w(w).flex_shrink_0(),
            None => base.pr_2().flex_grow(1.0).min_w(px(0.0)),
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
            .px_3()
            .py_1()
            .text_xs()
            .text_color(rgb(FG_SUBTLE))
            .border_b_1()
            .border_color(rgb(BORDER_MUTED))
            .child(self.col_head(SortField::Name, "Name", None, false, None, cx))
            .child(self.col_head(
                SortField::Size,
                "Size",
                Some(self.col_size_width),
                true,
                Some(Column::Size),
                cx,
            ))
            .child(self.col_head(
                SortField::Modified,
                "Date Modified",
                Some(self.col_date_width),
                false,
                Some(Column::Date),
                cx,
            ))
    }

    pub(crate) fn render_explorer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.entries().len();
        let count_text = match self.dir_query.status() {
            _ if self.open_remote.is_none() => String::new(),
            Status::Error(_) => String::new(),
            _ => format!("{count} items"),
        };

        let making_new = self.prompt.as_ref().is_some_and(|p| p.read(cx).target.is_none());
        let body = if self.open_remote.is_none() {
            centered("Select a remote to browse", FG_SUBTLE).into_any_element()
        } else if matches!(self.dir_query.status(), Status::Loading) {
            loading_view().into_any_element()
        } else if let Status::Error(message) = self.dir_query.status() {
            self.render_error(message.clone(), cx).into_any_element()
        } else if count == 0 && !making_new {
            centered("This folder is empty", FG_SUBTLE).into_any_element()
        } else {
            uniform_list(
                "entries",
                count,
                cx.processor(|this, range: Range<usize>, _window, cx| {
                    let focused = this.pane == Pane::Explorer;
                    range
                        .filter_map(|ix| this.entries().get(ix).map(|e| (ix, e.clone())))
                        .map(|(ix, entry)| {
                            // Renaming this row: swap in the inline editor.
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
                            let ctx_entry = entry.clone();
                            let drag = DraggedEntry {
                                path: entry.path.clone(),
                                name: name.clone(),
                                is_dir,
                                count: if selected { this.selected.len().max(1) } else { 1 },
                            };
                            let drop_path = entry.path.clone();
                            list_item(ix, selected, focused)
                                // Flush columns (no inter-column gap) so cells line up
                                // with the header and the resize dividers.
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
                                // Folders accept a drop: move (or copy with Option) into them.
                                .when(is_dir, |r| {
                                    let dst = this.open_remote.clone().unwrap_or_default();
                                    this.entry_drop_target(r, dst, drop_path, cx)
                                })
                                .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                                    this.pane = Pane::Explorer;
                                    this.context = None;
                                    this.prompt = None;
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
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.pane = Pane::Explorer;
                                        this.bg_menu = None;
                                        // Right-click outside the selection narrows to that row.
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
                                        .flex_grow(1.0)
                                        .min_w(px(0.0))
                                        .pr_2()
                                        .tooltip(tooltip_text(name.clone()))
                                        .when(is_dir, |r| r.child(file_icon(true)))
                                        .child(div().truncate().child(name)),
                                )
                                .child(
                                    h_flex()
                                        .w(this.col_size_width)
                                        .flex_shrink_0()
                                        .px_2()
                                        .justify_end()
                                        .text_xs()
                                        .text_color(rgb(FG_MUTED))
                                        .child(size_label),
                                )
                                .child(
                                    div()
                                        .w(this.col_date_width)
                                        .flex_shrink_0()
                                        .px_2()
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

        // A new-folder edit (no rename target) leads the list.
        let new_item = making_new && self.open_remote.is_some();
        let show_table = self.open_remote.is_some()
            && !matches!(self.dir_query.status(), Status::Loading | Status::Error(_));
        // Right-click on empty space opens the background menu.
        let body_area = v_flex()
            .flex_1()
            .min_h(px(0.0))
            .on_drag_move(cx.listener(Self::on_column_drag))
            .when(show_table, |el| el.child(self.column_header(cx)))
            .when(new_item, |el| el.child(self.prompt.as_ref().unwrap().clone()))
            .child(body)
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
            },
        );

        v_flex()
            .flex_1()
            .bg(rgb(INSET))
            .child(
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
                            .child(count_text)
                            .child(
                                icon_button("toggle-preview", "icons/sidebar_right.svg")
                                    .when(self.preview_open, |b| b.bg(rgba(SELECT_MUTED)))
                                    .tooltip(tooltip_text("Preview (Space)"))
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.toggle_preview(&TogglePreview, window, cx)
                                    })),
                            ),
                    ),
            )
            .child(body_area)
    }

    /// The "rspace" wordmark: bold foreground text.
    fn render_brand(&self) -> impl IntoElement {
        div().text_sm().font_weight(gpui::FontWeight::BOLD).text_color(rgb(FG)).child("rspace")
    }

    pub(crate) fn render_title_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lead = if window.is_fullscreen() { 12.0 } else { TITLE_BAR_LEAD };
        h_flex()
            .h(px(TITLE_BAR_H))
            .flex_shrink_0()
            .w_full()
            .pl(px(lead))
            .pr_2()
            .justify_between()
            .bg(rgb(INSET))
            .border_b_1()
            .border_color(rgb(BORDER_MUTED))
            .child(self.render_brand())
            .child(
                h_flex()
                    .id("settings-button")
                    .size(px(24.0))
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(OVERLAY)))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.settings_open = true;
                        cx.notify();
                    }))
                    .child(svg().path("icons/settings.svg").size(px(16.0)).text_color(rgb(FG_MUTED))),
            )
    }


    /// Dim full-screen backdrop holding a centered `card`; clicking outside runs
    /// `dismiss`. The card supplies its own `stop_propagation`.
    /// Dimmed full-screen backdrop with click-to-dismiss. `deferred_layer` draws
    /// it on a top z-layer (simple modals); a non-deferred overlay keeps its
    /// focusable controls in the tab-stop tree for native Tab (form modals).
    pub(crate) fn modal_overlay(
        &self,
        deferred_layer: bool,
        align_top: bool,
        dismiss: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        card: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let overlay = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
            // Pickers anchor near the top (Zed-style); dialogs center vertically.
            .map(|el| if align_top { el.items_start().pt(px(80.0)) } else { el.items_center() })
            .bg(rgba(0x0000_0099))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| dismiss(this, cx)),
            )
            .child(card);
        if deferred_layer {
            deferred(overlay).priority(3).into_any_element()
        } else {
            overlay.occlude().into_any_element()
        }
    }

}
