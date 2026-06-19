//! Rendering for the [`Explorer`] body: search bar, column header, and the
//! entry list. Navigation chrome (toolbar, breadcrumb) and the preview pane are
//! rendered by the workspace around this view.

use gpui::ExternalPaths;

use super::*;

impl Explorer {
    /// A folder cell that accepts a dragged entry, tinting on hover and emitting
    /// a [`ExplorerEvent::Drop`] (move, or copy when the modifier is held).
    fn drop_target(
        &self,
        el: Stateful<Div>,
        dst_remote: String,
        dst_dir: String,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        el.drag_over::<DraggedEntry>(|s, _, _, _| s.bg(rgba(ACCENT_SOFT)))
            .on_drop(cx.listener(move |_, dragged: &DraggedEntry, window, cx| {
                let copy = window.modifiers().alt;
                cx.emit(ExplorerEvent::Drop {
                    dragged: dragged.clone(),
                    dst_remote: dst_remote.clone(),
                    dst_dir: dst_dir.clone(),
                    copy,
                });
            }))
    }

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
                        this.workspace
                            .update(cx, |ws, cx| ws.reset_column(col, cx))
                            .ok();
                    }
                }))
                .child(div().w(px(1.0)).h(px(13.0)).bg(rgb(BORDER_MUTED))),
        )
    }

    fn column_header(&self, size_w: Pixels, date_w: Pixels, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(self.col_head(SortField::Size, "Size", Some(size_w), true, Some(Column::Size), cx))
            .child(self.col_head(SortField::Modified, "Date Modified", Some(date_w), false, Some(Column::Date), cx))
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
            .child(svg().path("icons/search.svg").size(px(14.0)).flex_shrink_0().text_color(rgb(FG_SUBTLE)))
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

    fn render_error(&self, message: String, _cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace.clone();
        let copy = message.clone();
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
                        .child(message),
                )
                .child(
                    h_flex().w_full().justify_end().child(
                        icon_button("copy-error", "icons/copy.svg")
                            .tooltip(tooltip_text("Copy error"))
                            .on_click(move |_, _, cx| {
                                let copy = copy.clone();
                                ws.update(cx, |ws, cx| {
                                    ws.copy_with_feedback(CopySource::Error, copy, cx)
                                })
                                .ok();
                            }),
                    ),
                ),
        )
    }
}

impl Render for Explorer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.rebuild_search_view();
        self.resolve_selection();

        let (size_w, date_w, prompt) = self
            .workspace
            .upgrade()
            .map(|ws| {
                let ws = ws.read(cx);
                (ws.col_size_width, ws.col_date_width, ws.prompt())
            })
            .unwrap_or((px(COL_SIZE), px(COL_DATE), None));
        let count = self.entries().len();
        let making_new = prompt.as_ref().is_some_and(|p| p.read(cx).target.is_none());
        let focused = self.focus.is_focused(window);

        let search_error = self
            .recursive_intent()
            .then(|| match self.search_query.status() {
                Status::Error(m) => Some(m.clone()),
                _ => None,
            })
            .flatten();

        let body = if matches!(self.dir_query.status(), Status::Loading) {
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
            let prompt = prompt.clone();
            uniform_list(
                "entries",
                count,
                cx.processor(move |this, range: Range<usize>, _window, cx| {
                    let matcher = Matcher::new(&this.search);
                    range
                        .filter_map(|ix| this.entries().get(ix).map(|e| (ix, e.clone())))
                        .map(|(ix, entry)| {
                            let renaming = prompt
                                .as_ref()
                                .is_some_and(|p| p.read(cx).target.as_deref() == Some(entry.path.as_str()));
                            if renaming {
                                return prompt.as_ref().unwrap().clone().into_any_element();
                            }
                            let selected = this.selected.contains(&entry.path);
                            let is_dir = entry.is_dir;
                            let size_label = if is_dir { "--".to_string() } else { human_size(entry.size) };
                            let date_label = human_date(&entry.mod_time);
                            let name = entry.name.clone();
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
                                    let dst = this.remote.clone().unwrap_or_default();
                                    this.drop_target(r, dst, drop_path, cx)
                                })
                                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
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
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|_, _: &MouseDownEvent, _, cx| cx.stop_propagation()),
                                )
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                                        cx.stop_propagation();
                                        this.focus.focus(window, cx);
                                        if !this.selected.contains(&ctx_entry.path) {
                                            this.select_only(ix);
                                        }
                                        cx.emit(ExplorerEvent::Context(ctx_entry.clone(), ev.position));
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
                                        .w(size_w)
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
                                        .w(date_w)
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

        let show_table = !matches!(self.dir_query.status(), Status::Loading | Status::Error(_));
        v_flex()
            .key_context("Explorer")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::select_all))
            .flex_1()
            .min_h(px(0.0))
            .min_w(px(0.0))
            .overflow_hidden()
            .on_drag_move(cx.listener(|this, e: &DragMoveEvent<DragColumn>, window, cx| {
                this.workspace.update(cx, |ws, cx| ws.on_column_drag(e, window, cx)).ok();
            }))
            .when(show_table && self.search_open, |el| el.child(self.search_bar(cx)))
            .when(show_table, |el| el.child(self.column_header(size_w, date_w, cx)))
            .when(making_new, |el| el.child(prompt.as_ref().unwrap().clone()))
            .child(v_flex().flex_1().min_h(px(0.0)).min_w(px(0.0)).child(body))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, ev: &MouseDownEvent, _, cx| {
                    cx.emit(ExplorerEvent::Background(ev.position));
                }),
            )
            // Click in empty space deselects (Finder-style). Rows stop propagation.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    this.clear_selection(cx);
                }),
            )
            // Files dragged from Finder upload into the open directory.
            .drag_over::<ExternalPaths>(|s, _, _, _| s.bg(rgba(ACCENT_SOFT)))
            .on_drop(cx.listener(|_, paths: &ExternalPaths, _, cx| {
                cx.emit(ExplorerEvent::Upload(paths.paths().to_vec()));
            }))
            .into_any_element()
    }
}
