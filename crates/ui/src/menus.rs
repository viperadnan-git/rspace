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
        self.menu_item_toned(label, icon, FG, FG_MUTED, cx, action)
    }

    /// A destructive menu item, tinted with the danger color.
    fn menu_item_danger(
        &self,
        label: &'static str,
        icon: &'static str,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        self.menu_item_toned(label, icon, DANGER, DANGER, cx, action)
    }

    fn menu_item_toned(
        &self,
        label: &'static str,
        icon: &'static str,
        text: u32,
        icon_color: u32,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        h_flex()
            .id(label)
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
            .child(label)
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

    pub(crate) fn render_context_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (entry, pos) = self.menus.context.clone().unwrap();
        let remote = self.active().open_remote.clone().unwrap_or_default();
        let mut items: Vec<AnyElement> = Vec::new();

        if entry.is_dir {
            let (e, r) = (entry.clone(), remote.clone());
            items.push(
                self.menu_item("Open", "icons/folder_open.svg", cx, move |this, _, cx| {
                    this.navigate(r.clone(), e.path.clone(), None, cx)
                })
                .into_any_element(),
            );
        }
        items.push(
            self.menu_item("Download", "icons/download.svg", cx, move |this, _, cx| {
                this.download_selected(cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item("Copy", "icons/copy.svg", cx, move |this, _, cx| {
                this.set_clipboard(TransferMode::Copy, cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item("Cut", "icons/scissors.svg", cx, move |this, _, cx| {
                this.set_clipboard(TransferMode::Move, cx)
            })
            .into_any_element(),
        );
        if self.clipboard.is_some() {
            // Paste into the folder when the target is one; a file pastes alongside
            // it, into the current directory (modern file-explorer behaviour).
            let into = entry.is_dir.then(|| entry.path.clone());
            items.push(
                self.menu_item("Paste", "icons/clipboard.svg", cx, move |this, _, cx| match &into {
                    Some(dir) => this.paste_clipboard_into(dir.clone(), cx),
                    None => this.paste_clipboard(cx),
                })
                .into_any_element(),
            );
        }
        let (e_rn, r_rn) = (entry.clone(), remote.clone());
        items.push(
            self.menu_item("Rename", "icons/edit.svg", cx, move |this, _, cx| {
                this.begin_rename(r_rn.clone(), e_rn.clone(), cx)
            })
            .into_any_element(),
        );
        let (e_cp, r_cp) = (entry.clone(), remote.clone());
        items.push(
            self.menu_item("Copy path", "icons/copy.svg", cx, move |this, _, cx| {
                this.copy_to_clipboard(format!("{}:{}", r_cp, e_cp.path), cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item_danger("Delete", "icons/trash.svg", cx, |this, _, cx| {
                this.request_delete_selected(cx)
            })
            .into_any_element(),
        );

        self.popover("context-menu", pos, gpui::Anchor::TopLeft, items, cx)
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

    pub(crate) fn render_task_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (data, pos) = self.menus.task_menu.clone().unwrap();
        let mut items: Vec<AnyElement> = Vec::new();
        // Reveal endpoints in the explorer (source first, then destination).
        for (label, target) in [("Open source", data.targets.first()), ("Open destination", data.targets.get(1))] {
            if let Some(target) = target.cloned() {
                items.push(
                    self.menu_item(label, "icons/folder_open.svg", cx, move |this, _, cx| {
                        this.reveal_target_in_explorer(target.clone(), cx)
                    })
                    .into_any_element(),
                );
            }
        }
        if !data.command.is_empty() {
            let command = data.command.clone();
            items.push(
                self.menu_item("Copy command", "icons/copy.svg", cx, move |this, _, cx| {
                    this.copy_to_clipboard(command.clone(), cx)
                })
                .into_any_element(),
            );
        }
        for (label, target) in [("Copy source path", data.targets.first()), ("Copy destination path", data.targets.get(1))] {
            if let Some(target) = target.cloned() {
                let path = format!("{}:{}", target.remote, target.path);
                items.push(
                    self.menu_item(label, "icons/copy.svg", cx, move |this, _, cx| {
                        this.copy_to_clipboard(path.clone(), cx)
                    })
                    .into_any_element(),
                );
            }
        }
        let id = data.job_id;
        if data.running {
            items.push(
                self.menu_item_danger("Cancel", "icons/x.svg", cx, move |this, _, cx| this.request_cancel_job(id, cx))
                    .into_any_element(),
            );
        }
        if data.can_retry {
            items.push(
                self.menu_item("Retry", "icons/refresh.svg", cx, move |this, _, cx| this.retry_job(id, cx))
                    .into_any_element(),
            );
        }
        if data.can_remove {
            items.push(
                self.menu_item_danger("Remove", "icons/trash.svg", cx, move |this, _, cx| this.clear_job(id, cx))
                    .into_any_element(),
            );
        }
        self.popover("task-menu", pos, gpui::Anchor::TopLeft, items, cx)
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
