//! Explorer, sidebar, breadcrumb, chrome, and dialog views.

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
            row = row.child(
                div()
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
                    .child(label),
            );
        }
        row.child(self.copy_button("copy-path", CopySource::Path, self.copy_text(), "Copy path", cx).ml_1())
    }

    // Overlay on the sidebar's right border; takes no layout space, so
    // `deferred` paints/hit-tests it over the next pane.
    fn resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        deferred(
            div()
                .id("sidebar-resize")
                .absolute()
                .top(px(0.0))
                .right(px(-RESIZE_HANDLE_W / 2.0))
                .w(px(RESIZE_HANDLE_W))
                .h_full()
                .cursor_col_resize()
                .occlude()
                .on_drag(DragSidebar, |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| DragSidebar)
                })
                .on_click(cx.listener(|this, e: &ClickEvent, _, cx| {
                    if e.click_count() >= 2 {
                        this.sidebar_width = px(SIDEBAR_W);
                        cx.notify();
                    }
                })),
        )
    }

    fn render_sort(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = format!("{} {}", self.sort_field.label(), sort_arrow(self.sort_order));
        h_flex()
            .id("sort-button")
            .gap_1()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_color(rgb(FG_MUTED))
            .hover(|s| s.bg(rgba(OVERLAY)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.sort_menu_open = !this.sort_menu_open;
                cx.notify();
            }))
            .child(label)
            .when(self.sort_menu_open, |b| {
                b.child(
                    deferred(
                        anchored()
                            .anchor(Anchor::TopRight)
                            .snap_to_window_with_margin(px(8.0))
                            .child(self.sort_menu(cx)),
                    )
                    .priority(1),
                )
            })
    }

    fn sort_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("sort-menu")
            .occlude()
            .mt(px(22.0))
            .min_w(px(160.0))
            .p_1()
            .rounded_md()
            .bg(rgb(ELEVATED))
            .border_1()
            .border_color(rgb(BORDER_MUTED))
            .shadow_lg()
            .text_color(rgb(FG))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.sort_menu_open = false;
                cx.notify();
            }))
            .child(self.sort_item(SortField::Name, cx))
            .child(self.sort_item(SortField::Size, cx))
            .child(self.sort_item(SortField::Modified, cx))
    }

    fn sort_item(&self, field: SortField, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.sort_field == field;
        let arrow = if active { sort_arrow(self.sort_order) } else { "" };
        h_flex()
            .id(field.label())
            .w_full()
            .justify_between()
            .gap_4()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_color(if active { rgb(FG) } else { rgb(FG_MUTED) })
            .hover(|s| s.bg(rgba(SELECT_MUTED)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.choose_sort(field, cx)))
            .child(field.label())
            .child(div().text_color(rgb(ACCENT)).child(arrow))
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
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.load_remote(ix, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    this.remote_menu = Some((menu_name.clone(), ev.position));
                    cx.notify();
                }),
            )
            .when(pinned, |r| {
                r.child(svg().path("icons/pin.svg").size(px(12.0)).flex_shrink_0().text_color(rgb(ACCENT)))
            })
            .child(
                div()
                    .flex_grow(1.0)
                    .min_w(px(0.0))
                    .truncate()
                    .text_color(rgb(FG))
                    .child(remote.name.clone()),
            )
            .child(div().text_xs().flex_shrink_0().text_color(rgb(FG_SUBTLE)).child(remote.kind.clone()));

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
            .child(self.resize_handle(cx))
            .child(div().px_3().py_2().text_xs().text_color(rgb(FG_SUBTLE)).child("REMOTES"))
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

    /// Inline text field for new-folder / rename, styled to sit in the file list.
    fn inline_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.prompt.as_ref().unwrap();
        let (value, placeholder, icon_dir) = (p.value.clone(), p.placeholder.clone(), p.icon_dir);
        let empty = value.is_empty();
        // Rename pins to the end so the caret stays visible on long names; a new
        // item grows from the left.
        let pin_end = p.target.is_some();
        let caret = || div().w(px(1.5)).h(px(15.0)).flex_shrink_0().bg(rgb(ACCENT));
        h_flex()
            .id("inline-editor")
            .key_context("modal Prompt")
            .track_focus(&self.dialog_focus)
            .on_action(cx.listener(Self::prompt_submit))
            .on_action(cx.listener(Self::prompt_cancel))
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _, cx| this.prompt_key(ev, cx)))
            .w_full()
            .gap_2()
            .px_3()
            .py_1()
            .items_center()
            .bg(rgba(SELECT))
            .border_1()
            .border_color(rgb(ACCENT))
            .child(file_icon(icon_dir))
            .child(
                // overflow_hidden + flex_shrink_0 text = single line that clips
                // instead of truncating; justify_end keeps the tail/caret in view.
                h_flex()
                    .flex_grow(1.0)
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .items_center()
                    .when(pin_end, |f| f.justify_end())
                    .when(empty, |e| {
                        e.child(caret())
                            .child(div().flex_shrink_0().text_color(rgb(FG_SUBTLE)).child(placeholder))
                    })
                    .when(!empty, |e| {
                        e.child(div().flex_shrink_0().text_color(rgb(FG)).child(value)).child(caret())
                    }),
            )
    }

    pub(crate) fn render_explorer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.entries().len();
        let count_text = match self.dir_query.status() {
            _ if self.open_remote.is_none() => String::new(),
            Status::Error(_) => String::new(),
            _ => format!("{count} items"),
        };

        let body = if self.open_remote.is_none() {
            centered("Select a remote to browse", FG_SUBTLE).into_any_element()
        } else if matches!(self.dir_query.status(), Status::Loading) {
            loading_view().into_any_element()
        } else if let Status::Error(message) = self.dir_query.status() {
            self.render_error(message.clone(), cx).into_any_element()
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
                            if this.prompt.as_ref().and_then(|p| p.target.as_deref())
                                == Some(entry.path.as_str())
                            {
                                return this.inline_editor(cx).into_any_element();
                            }
                            let selected = this.selected.contains(&entry.path);
                            let is_cursor = ix == this.entry_sel;
                            let is_dir = entry.is_dir;
                            let size_label = human_size(entry.size);
                            let name = entry.name.clone();
                            let ctx_entry = entry.clone();
                            // Cursor ring only earns its place within a multi-selection
                            // (single-select already reads from the row background).
                            let ring = is_cursor && focused && this.selected.len() > 1;
                            list_item(ix, selected, focused)
                                .border_1()
                                .border_color(if ring { rgb(ACCENT) } else { rgba(0x0000_0000) })
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
                                        .tooltip(tooltip_text(name.clone()))
                                        .child(file_icon(is_dir))
                                        .child(div().truncate().child(name)),
                                )
                                .child(if is_dir {
                                    div()
                                } else {
                                    div().text_xs().text_color(rgb(FG_MUTED)).child(size_label)
                                })
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
        let new_item = self.prompt.as_ref().is_some_and(|p| p.target.is_none()) && self.open_remote.is_some();
        // Right-click on empty space opens the background menu.
        let body_area = v_flex()
            .flex_1()
            .min_h(px(0.0))
            .when(new_item, |el| el.child(self.inline_editor(cx)))
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
            .bg(rgb(CANVAS))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_between()
                    .pl_1()
                    .pr_3()
                    .py_1()
                    .border_b_1()
                    .border_color(rgb(BORDER_MUTED))
                    .child(
                        h_flex()
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
                            .when(self.open_remote.is_some(), |el| {
                                el.child(self.render_sort(cx))
                            }),
                    ),
            )
            .child(body_area)
    }

    pub(crate) fn render_title_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lead = if window.is_fullscreen() { 12.0 } else { TITLE_BAR_LEAD };
        h_flex()
            .h(px(TITLE_BAR_H))
            .flex_shrink_0()
            .w_full()
            .pl(px(lead))
            .pr_2()
            .justify_end()
            .bg(rgb(INSET))
            .border_b_1()
            .border_color(rgb(BORDER_MUTED))
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
    pub(crate) fn modal_overlay(
        &self,
        dismiss: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        card: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        deferred(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .justify_center()
                .items_center()
                .bg(rgba(0x0000_0099))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| dismiss(this, cx)),
                )
                .child(card),
        )
        .priority(3)
    }

    /// Base for a centered modal card: elevated surface that swallows clicks.
    pub(crate) fn modal_card(&self, id: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        v_flex()
            .id(id)
            .p_5()
            .rounded_lg()
            .bg(rgb(ELEVATED))
            .border_1()
            .border_color(rgb(BORDER_MUTED))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, cx.listener(|_, _: &MouseDownEvent, _, cx| cx.stop_propagation()))
    }

    pub(crate) fn render_confirm(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = self.confirm.as_ref().unwrap();
        let (title, message, label, danger) =
            (c.title.clone(), c.message.clone(), c.confirm_label.clone(), c.danger);
        let accent = if danger { DANGER } else { ACCENT };
        let card = self
            .modal_card("confirm-card", cx)
            .key_context("modal Confirm")
            .track_focus(&self.dialog_focus)
            .on_action(cx.listener(Self::confirm_accept))
            .w(px(400.0))
            .gap_4()
            .child(div().text_lg().text_color(rgb(FG)).child(title))
            .child(div().text_sm().text_color(rgb(FG_MUTED)).child(message))
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        text_button("confirm-cancel", "Cancel")
                            .text_color(rgb(FG))
                            .hover(|s| s.bg(rgba(OVERLAY)))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.dismiss_confirm(cx))),
                    )
                    .child(
                        text_button("confirm-accept", label)
                            .bg(rgb(accent))
                            .text_color(rgb(0xffffff))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.run_confirm(cx))),
                    ),
            );
        self.modal_overlay(|this, cx| this.dismiss_confirm(cx), card, cx)
    }

}
