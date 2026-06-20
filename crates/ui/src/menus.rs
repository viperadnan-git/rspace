//! Right-click context menus and the shared popover surface.

use super::*;

impl Workspace {
    pub(crate) fn menu_item(
        &self,
        label: &'static str,
        icon: &'static str,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        self.menu_item_toned(label, label, icon, FG, FG_MUTED, cx, action)
    }

    /// A destructive menu item, tinted with the danger color.
    fn menu_item_danger(
        &self,
        label: &'static str,
        icon: &'static str,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        self.menu_item_toned(label, label, icon, DANGER, DANGER, cx, action)
    }

    /// Render a declarative [`MenuSpec`] into a popover at `pos`. The single source
    /// for selection-aware menus (entries, tasks): callers describe rows; rows wire
    /// to `menu_item_toned`, separators to a hairline. Each item closes the menu.
    fn render_menu(
        &self,
        id: &'static str,
        pos: Point<Pixels>,
        spec: MenuSpec,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // Resolve separators structurally: a boundary becomes a divider only with
        // an item on both sides, so a divider never leads, trails, or doubles up no
        // matter which conditional groups are empty.
        let mut items: Vec<AnyElement> = Vec::with_capacity(spec.rows.len());
        let mut pending_divider = false;
        for row in spec.rows {
            match row {
                MenuRow::Separator => pending_divider = !items.is_empty(),
                MenuRow::Item { id, label, icon, danger, action } => {
                    if std::mem::take(&mut pending_divider) {
                        items.push(div().my_1().h(px(1.0)).bg(rgb(BORDER_MUTED)).into_any_element());
                    }
                    let (text, icon_color) = if danger { (DANGER, DANGER) } else { (FG, FG_MUTED) };
                    items.push(
                        self.menu_item_toned(id, label, icon, text, icon_color, cx, move |this, w, cx| {
                            action(this, w, cx)
                        })
                        .into_any_element(),
                    );
                }
            }
        }
        self.popover(id, pos, gpui::Anchor::TopLeft, items, cx)
    }

    fn menu_item_toned(
        &self,
        id: impl Into<gpui::ElementId>,
        label: impl Into<SharedString>,
        icon: &'static str,
        text: u32,
        icon_color: u32,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        h_flex()
            .id(id.into())
            .w_full()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_color(rgb(text))
            .hover(|s| s.bg(rgba(OVERLAY)))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                action(this, window, cx);
                this.close_menus();
                cx.notify();
            }))
            .child(svg().path(icon).size(rem(15.0)).flex_shrink_0().text_color(rgb(icon_color)))
            .child(label.into())
    }

    /// Close every transient popover.
    pub(crate) fn close_menus(&mut self) {
        self.menus = Menus::default();
    }

    /// The elevated popover card: occludes content behind it and dismisses on an
    /// outside mouse-down. Used both free-anchored (menus) and attached to a
    /// trigger element (the status-bar daemon button).
    pub(crate) fn popover_surface(
        &self,
        id: &'static str,
        items: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        v_flex()
            .id(id)
            .occlude()
            .min_w(rem(180.0))
            .p_1()
            .rounded_md()
            .bg(rgb(ELEVATED))
            .border_1()
            .border_color(rgb(BORDER_MUTED))
            .shadow_lg()
            .text_color(rgb(FG))
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.close_menus();
                cx.notify();
            }))
            .children(items)
    }

    /// A [`popover_surface`] free-anchored at `pos` (whose `anchor` corner sits
    /// there), for the right-click menus.
    pub(crate) fn popover(
        &self,
        id: &'static str,
        pos: Point<Pixels>,
        anchor: gpui::Anchor,
        items: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let surface = self.popover_surface(id, items, cx);
        deferred(anchored().position(pos).anchor(anchor).snap_to_window_with_margin(px(8.0)).child(surface))
            .priority(2)
    }

    /// The entry context menu, adapted to the selection. Selection-wide actions
    /// (Download, Copy, Cut, Delete) always show and pluralize with a count;
    /// single-target actions (Open, Rename, Paste-into) appear only for one entry.
    /// `entry` is the right-clicked row — the right-click handler has already made
    /// it the lone selection if it wasn't already part of one.
    pub(crate) fn render_context_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (entry, pos) = self.menus.context.clone().unwrap();
        let remote = self.active().open_remote.clone().unwrap_or_default();
        let sel = self.selected_entries(cx);
        let n = sel.len().max(1);
        let single = n == 1;
        let has_clip = self.clipboard.is_some();
        let count = |verb: &str| if single { verb.to_string() } else { format!("{verb} {n} items") };
        let paths: Vec<String> = if single {
            vec![format!("{}:{}", remote, entry.path)]
        } else {
            sel.iter().map(|e| format!("{}:{}", remote, e.path)).collect()
        };

        let spec = MenuSpec::new()
            .when(single && entry.is_dir, |m| {
                let (e, r) = (entry.clone(), remote.clone());
                m.item("ctx-open", "Open", "icons/folder_open.svg", move |this, _, cx| {
                    this.navigate(r.clone(), e.path.clone(), None, cx)
                })
            })
            .item("ctx-download", count("Download"), "icons/download.svg", |this, _, cx| {
                this.download_selected(cx)
            })
            .item("ctx-copy", count("Copy"), "icons/copy.svg", |this, _, cx| {
                this.set_clipboard(TransferMode::Copy, cx)
            })
            .item("ctx-cut", count("Cut"), "icons/scissors.svg", |this, _, cx| {
                this.set_clipboard(TransferMode::Move, cx)
            })
            // Paste targets a single folder (into it) or the current dir; it has no
            // meaning while a multi-selection is the operand.
            .when(has_clip && single, |m| {
                let into = entry.is_dir.then(|| entry.path.clone());
                m.item("ctx-paste", "Paste", "icons/clipboard.svg", move |this, _, cx| match &into {
                    Some(dir) => this.paste_clipboard_into(dir.clone(), cx),
                    None => this.paste_clipboard(cx),
                })
            })
            .separator()
            .when(single, |m| {
                let (e, r) = (entry.clone(), remote.clone());
                m.item("ctx-rename", "Rename", "icons/edit.svg", move |this, _, cx| {
                    this.begin_rename(r.clone(), e.clone(), cx)
                })
            })
            .item("ctx-copy-path", if single { "Copy path" } else { "Copy paths" }, "icons/copy.svg", move |this, _, cx| {
                this.copy_to_clipboard(paths.join("\n"), cx)
            })
            .separator()
            .danger("ctx-delete", count("Delete"), "icons/trash.svg", |this, _, cx| {
                this.request_delete_selected(cx)
            });

        self.render_menu("context-menu", pos, spec, cx)
    }

    pub(crate) fn render_tab_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, pos) = self.menus.tab_menu.unwrap();
        let pinned = self.is_tab_pinned(id);
        let mut items: Vec<AnyElement> = Vec::new();
        items.push(
            self.menu_item(if pinned { "Unpin Tab" } else { "Pin Tab" }, "icons/pin.svg", cx, move |this, _, cx| {
                this.toggle_pin_tab(id, cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item("Close", "icons/x.svg", cx, move |this, window, cx| {
                this.close_tab_id(id, window, cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item("Close Others", "icons/x.svg", cx, move |this, window, cx| {
                this.close_other_tabs(id, window, cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item("Close to the Right", "icons/x.svg", cx, move |this, window, cx| {
                this.close_tabs_to_right(id, window, cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item("Close All", "icons/trash.svg", cx, move |this, window, cx| {
                this.close_all_tabs(window, cx)
            })
            .into_any_element(),
        );
        self.popover("tab-menu", pos, gpui::Anchor::TopLeft, items, cx)
    }

    pub(crate) fn render_remote_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (name, pos) = self.menus.remote_menu.clone().unwrap();
        let pinned = self.is_pinned(&name);
        let mut items: Vec<AnyElement> = Vec::new();

        let open_name = name.clone();
        items.push(
            self.menu_item("Open", "icons/folder_open.svg", cx, move |this, _, cx| {
                if let Some(ix) = this.ordered_remotes().iter().position(|r| r.name == open_name) {
                    this.load_remote(ix, cx);
                }
            })
            .into_any_element(),
        );

        let newtab_name = name.clone();
        items.push(
            self.menu_item("Open in new tab", "icons/plus.svg", cx, move |this, window, cx| {
                this.open_remote_in_new_tab(newtab_name.clone(), window, cx)
            })
            .into_any_element(),
        );

        let mounted = self.mounted.contains(&name);
        let mount_name = name.clone();
        items.push(
            self.menu_item(
                if mounted { "Unmount" } else { "Mount" },
                "icons/hard_drive.svg",
                cx,
                move |this, _, cx| this.toggle_mount(mount_name.clone(), cx),
            )
            .into_any_element(),
        );
        if mounted {
            let reveal_name = name.clone();
            items.push(
                self.menu_item("Reveal in Finder", "icons/folder_open.svg", cx, move |this, _, cx| {
                    this.reveal_mount(&reveal_name, cx)
                })
                .into_any_element(),
            );
        }
        let opts_name = name.clone();
        items.push(
            self.menu_item("Mount options\u{2026}", "icons/settings.svg", cx, move |this, _, cx| {
                this.begin_mount_options(opts_name.clone(), cx)
            })
            .into_any_element(),
        );

        let pin_name = name.clone();
        let (pin_label, pin_icon) = if pinned { ("Unpin", "icons/pin.svg") } else { ("Pin", "icons/pin.svg") };
        items.push(
            self.menu_item(pin_label, pin_icon, cx, move |this, _, cx| {
                this.toggle_pin(pin_name.clone(), cx)
            })
            .into_any_element(),
        );

        if pinned {
            let up_name = name.clone();
            let down_name = name.clone();
            items.push(
                self.menu_item("Move up", "icons/chevron_up.svg", cx, move |this, _, cx| {
                    this.move_pinned(&up_name, true, cx)
                })
                .into_any_element(),
            );
            items.push(
                self.menu_item("Move down", "icons/chevron_down.svg", cx, move |this, _, cx| {
                    this.move_pinned(&down_name, false, cx)
                })
                .into_any_element(),
            );
        }

        let edit_name = name.clone();
        items.push(
            self.menu_item("Edit remote", "icons/edit.svg", cx, move |this, _, cx| {
                this.begin_edit_remote(edit_name.clone(), cx)
            })
            .into_any_element(),
        );

        let del_name = name.clone();
        items.push(
            self.menu_item_danger("Delete remote", "icons/trash.svg", cx, move |this, _, cx| {
                this.request_delete_remote(del_name.clone(), cx)
            })
            .into_any_element(),
        );

        self.popover("remote-menu", pos, gpui::Anchor::TopLeft, items, cx)
    }

    /// The task-row menu, over the current task selection. One job → the full
    /// single-row menu (reveal/copy endpoints + cancel/retry/remove); many → bulk
    /// actions that fold over the selection (retry failed, cancel running, remove
    /// finished, copy commands). Jobs are snapshotted live so the menu reflects
    /// state at open time even though it's described declaratively.
    pub(crate) fn render_task_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (ids, pos) = self.menus.task_menu.clone().unwrap();
        let sel: Vec<TaskSnap> = {
            let jobs = self.jobs.read(cx);
            ids.iter()
                .filter_map(|id| jobs.items().iter().find(|j| j.id == *id))
                .map(|j| TaskSnap {
                    id: j.id,
                    command: j.command.clone(),
                    targets: j.targets.clone(),
                    running: !j.done,
                    can_retry: j.done && j.error.is_some(),
                    can_remove: j.done,
                })
                .collect()
        };

        let spec = if let [t] = sel.as_slice() {
            let (id, command, targets) = (t.id, t.command.clone(), t.targets.clone());
            let (src, dst) = (targets.first().cloned(), targets.get(1).cloned());
            MenuSpec::new()
                .when(src.is_some(), |m| {
                    let t = src.clone().unwrap();
                    m.item("task-open-src", "Open source", "icons/folder_open.svg", move |this, _, cx| {
                        this.reveal_target_in_explorer(t.clone(), cx)
                    })
                })
                .when(dst.is_some(), |m| {
                    let t = dst.clone().unwrap();
                    m.item("task-open-dst", "Open destination", "icons/folder_open.svg", move |this, _, cx| {
                        this.reveal_target_in_explorer(t.clone(), cx)
                    })
                })
                .when(!command.is_empty(), |m| {
                    m.item("task-copy-cmd", "Copy command", "icons/copy.svg", move |this, _, cx| {
                        this.copy_to_clipboard(command.clone(), cx)
                    })
                })
                .when(src.is_some(), |m| {
                    let p = src.map(|t| format!("{}:{}", t.remote, t.path)).unwrap_or_default();
                    m.item("task-copy-src", "Copy source path", "icons/copy.svg", move |this, _, cx| {
                        this.copy_to_clipboard(p.clone(), cx)
                    })
                })
                .when(dst.is_some(), |m| {
                    let p = dst.map(|t| format!("{}:{}", t.remote, t.path)).unwrap_or_default();
                    m.item("task-copy-dst", "Copy destination path", "icons/copy.svg", move |this, _, cx| {
                        this.copy_to_clipboard(p.clone(), cx)
                    })
                })
                .separator()
                .when(t.running, |m| m.danger("task-cancel", "Cancel", "icons/x.svg", move |this, _, cx| this.request_cancel_job(id, cx)))
                .when(t.can_retry, |m| m.item("task-retry", "Retry", "icons/refresh.svg", move |this, _, cx| this.retry_job(id, cx)))
                .when(t.can_remove, |m| m.danger("task-remove", "Remove", "icons/trash.svg", move |this, _, cx| this.clear_job(id, cx)))
        } else {
            let retry_n = sel.iter().filter(|t| t.can_retry).count();
            let cancel_n = sel.iter().filter(|t| t.running).count();
            let remove_n = sel.iter().filter(|t| t.can_remove).count();
            let commands: Vec<String> = sel.iter().filter(|t| !t.command.is_empty()).map(|t| t.command.clone()).collect();
            MenuSpec::new()
                .when(retry_n > 0, |m| {
                    let ids = ids.clone();
                    m.item("task-retry-all", format!("Retry {retry_n}"), "icons/refresh.svg", move |this, _, cx| {
                        this.retry_selected_tasks(&ids, cx)
                    })
                })
                .when(cancel_n > 0, |m| {
                    let ids = ids.clone();
                    m.danger("task-cancel-all", format!("Cancel {cancel_n}"), "icons/x.svg", move |this, _, cx| {
                        this.cancel_selected_tasks(ids.clone(), cx)
                    })
                })
                .when(!commands.is_empty(), |m| {
                    m.item("task-copy-cmds", "Copy commands", "icons/copy.svg", move |this, _, cx| {
                        this.copy_to_clipboard(commands.join("\n"), cx)
                    })
                })
                .separator()
                .when(remove_n > 0, |m| {
                    let ids = ids.clone();
                    m.danger("task-remove-all", format!("Remove {remove_n}"), "icons/trash.svg", move |this, _, cx| {
                        this.remove_selected_tasks(&ids, cx)
                    })
                })
        };
        self.render_menu("task-menu", pos, spec, cx)
    }

    pub(crate) fn render_bg_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let pos = self.menus.bg_menu.unwrap();
        let mut items: Vec<AnyElement> = Vec::new();
        items.push(
            self.menu_item("New folder", "icons/new_folder.svg", cx, |this, _, cx| this.begin_new_folder(cx))
                .into_any_element(),
        );
        items.push(
            self.menu_item("Upload", "icons/upload.svg", cx, |this, _, cx| this.begin_upload(cx))
                .into_any_element(),
        );
        if self.clipboard.is_some() {
            items.push(
                self.menu_item("Paste", "icons/clipboard.svg", cx, |this, _, cx| this.paste_clipboard(cx))
                    .into_any_element(),
            );
        }
        items.push(
            self.menu_item("Refresh", "icons/refresh.svg", cx, |this, _, cx| this.force_reload_entries(cx))
                .into_any_element(),
        );
        let dir_path = self.copy_text();
        items.push(
            self.menu_item("Copy path", "icons/copy.svg", cx, move |this, _, cx| {
                this.copy_to_clipboard(dir_path.clone(), cx)
            })
            .into_any_element(),
        );
        self.popover("bg-menu", pos, gpui::Anchor::TopLeft, items, cx)
    }
}

/// A handler bound to a menu row; runs against the workspace when chosen.
type MenuAction = Box<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static>;

enum MenuRow {
    Item { id: gpui::ElementId, label: SharedString, icon: &'static str, danger: bool, action: MenuAction },
    Separator,
}

/// A declarative context-menu, built fluently (Zed `ContextMenu`-style) and
/// rendered by [`Workspace::render_menu`]. Keeping rows as data — rather than
/// pushing pre-rendered elements — lets a caller compose a menu from the live
/// selection (count-aware labels, conditional rows) without touching styling.
#[derive(Default)]
pub(crate) struct MenuSpec {
    rows: Vec<MenuRow>,
}

impl MenuSpec {
    fn new() -> Self {
        Self::default()
    }

    fn row(mut self, id: impl Into<gpui::ElementId>, label: impl Into<SharedString>, icon: &'static str, danger: bool, action: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static) -> Self {
        self.rows.push(MenuRow::Item { id: id.into(), label: label.into(), icon, danger, action: Box::new(action) });
        self
    }

    fn item(self, id: impl Into<gpui::ElementId>, label: impl Into<SharedString>, icon: &'static str, action: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static) -> Self {
        self.row(id, label, icon, false, action)
    }

    fn danger(self, id: impl Into<gpui::ElementId>, label: impl Into<SharedString>, icon: &'static str, action: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static) -> Self {
        self.row(id, label, icon, true, action)
    }

    /// Append `f`'s rows only when `cond` holds — the conditional-row primitive.
    fn when(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond { f(self) } else { self }
    }

    /// A group boundary. [`Workspace::render_menu`] draws it as a divider only when
    /// both sides have items, so callers add one between any two optional groups
    /// without tracking which were emitted.
    fn separator(mut self) -> Self {
        self.rows.push(MenuRow::Separator);
        self
    }
}

/// A live snapshot of a selected job, taken when the task menu opens.
struct TaskSnap {
    id: usize,
    command: String,
    targets: Vec<JobTarget>,
    running: bool,
    can_retry: bool,
    can_remove: bool,
}
