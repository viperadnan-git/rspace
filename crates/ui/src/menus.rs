//! Right-click context menus and the shared popover surface.

use super::*;

impl Workspace {
    /// Close every transient popover.
    pub(crate) fn close_menus(&mut self) {
        self.menu = None;
    }

    /// Open `spec` as a keyboard-navigable menu anchored at `pos`. The single
    /// entry point for every right-click menu.
    pub(crate) fn open_menu(&mut self, spec: MenuSpec, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let menu = cx.new(|cx| ContextMenu::new(spec, weak, cx));
        let sub = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.close_menus();
            cx.notify();
        });
        self.menu = Some(ActiveMenu::Items(menu, pos, sub));
        cx.notify();
    }

    /// Render the one open menu/popover, if any. The rc/sync popovers render their
    /// bodies inline in the status bar; here they contribute only the backdrop.
    pub(crate) fn render_active_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        Some(match self.menu.as_ref()? {
            ActiveMenu::Items(menu, pos, _) => deferred(
                anchored()
                    .position(*pos)
                    .anchor(gpui::Anchor::TopLeft)
                    .snap_to_window_with_margin(px(8.0))
                    .child(menu.clone()),
            )
            .priority(2)
            .into_any_element(),
            ActiveMenu::RcPopover(..) | ActiveMenu::SyncPopover => {
                self.rc_popover_backdrop(cx).into_any_element()
            }
        })
    }

    /// Whether an open menu holds the keyboard; the focus-restore guard must
    /// leave those alone.
    pub(crate) fn menu_owns_focus(&self) -> bool {
        matches!(self.menu, Some(ActiveMenu::Items(..) | ActiveMenu::RcPopover(..)))
    }

    /// The open daemon popover's menu, for the status bar to render in place.
    pub(crate) fn rc_popover(&self) -> Option<Entity<ContextMenu>> {
        match &self.menu {
            Some(ActiveMenu::RcPopover(menu, _)) => Some(menu.clone()),
            _ => None,
        }
    }

    /// Build a menu for a caller that anchors it itself (the status-bar
    /// popovers), rather than at a cursor position.
    pub(crate) fn build_menu(
        &mut self,
        spec: MenuSpec,
        cx: &mut Context<Self>,
    ) -> (Entity<ContextMenu>, gpui::Subscription) {
        let weak = cx.entity().downgrade();
        let menu = cx.new(|cx| ContextMenu::new(spec, weak, cx));
        let sub = cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.close_menus();
            cx.notify();
        });
        (menu, sub)
    }

    pub(crate) fn sync_popover_open(&self) -> bool {
        matches!(self.menu, Some(ActiveMenu::SyncPopover))
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

    /// The entry context menu, adapted to the selection. Selection-wide actions
    /// (Download, Copy, Cut, Delete) always show and pluralize with a count;
    /// single-target actions (Open, Rename, Paste-into) appear only for one entry.
    /// `entry` is the right-clicked row — the right-click handler has already made
    /// it the lone selection if it wasn't already part of one.
    pub(crate) fn entry_menu_spec(&self, entry: Entry, cx: &mut Context<Self>) -> MenuSpec {
        let remote = self.open_remote(cx).unwrap_or_default();
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

        spec
    }

    pub(crate) fn tab_menu_spec(&self, id: usize) -> MenuSpec {
        let pinned = self.is_tab_pinned(id);
        let spec = MenuSpec::new()
            .item("tab-pin", if pinned { "Unpin Tab" } else { "Pin Tab" }, "icons/pin.svg", move |this, _, cx| this.toggle_pin_tab(id, cx))
            .item("tab-close", "Close", "icons/x.svg", move |this, w, cx| this.close_tab_id(id, w, cx))
            .item("tab-close-others", "Close Others", "icons/x.svg", move |this, w, cx| this.close_other_tabs(id, w, cx))
            .item("tab-close-right", "Close to the Right", "icons/x.svg", move |this, w, cx| this.close_tabs_to_right(id, w, cx))
            .item("tab-close-all", "Close All", "icons/trash.svg", move |this, w, cx| this.close_all_tabs(id, w, cx));
        spec
    }

    pub(crate) fn remote_menu_spec(&self, name: String) -> MenuSpec {
        let pinned = self.is_pinned(&name);
        let mounted = self.mounted.contains(&name);
        let spec = MenuSpec::new()
            .item("rm-open", "Open", "icons/folder_open.svg", {
                let name = name.clone();
                move |this, _, cx| {
                    if let Some(ix) = this.ordered_remotes().iter().position(|r| r.name == name) {
                        this.load_remote(ix, cx);
                    }
                }
            })
            .item("rm-newtab", "Open in new tab", "icons/plus.svg", {
                let name = name.clone();
                move |this, w, cx| this.open_remote_in_new_tab(name.clone(), w, cx)
            })
            .item("rm-mount", if mounted { "Unmount" } else { "Mount" }, "icons/hard_drive.svg", {
                let name = name.clone();
                move |this, _, cx| this.toggle_mount(name.clone(), cx)
            })
            .when(mounted, |m| {
                let name = name.clone();
                m.item("rm-reveal", "Reveal in Finder", "icons/folder_open.svg", move |this, _, cx| this.reveal_mount(&name, cx))
            })
            .item("rm-opts", "Mount options\u{2026}", "icons/settings.svg", {
                let name = name.clone();
                move |this, _, cx| this.begin_mount_options(name.clone(), cx)
            })
            .item("rm-pin", if pinned { "Unpin" } else { "Pin" }, "icons/pin.svg", {
                let name = name.clone();
                move |this, _, cx| this.toggle_pin(name.clone(), cx)
            })
            .when(pinned, |m| {
                let (up, down) = (name.clone(), name.clone());
                m.item("rm-up", "Move up", "icons/chevron_up.svg", move |this, _, cx| this.move_pinned(&up, true, cx))
                    .item("rm-down", "Move down", "icons/chevron_down.svg", move |this, _, cx| this.move_pinned(&down, false, cx))
            })
            .item("rm-edit", "Edit remote", "icons/edit.svg", {
                let name = name.clone();
                move |this, _, cx| this.begin_edit_remote(name.clone(), cx)
            })
            .danger("rm-delete", "Delete remote", "icons/trash.svg", {
                let name = name.clone();
                move |this, _, cx| this.request_delete_remote(name.clone(), cx)
            });
        spec
    }

    /// The task-row menu, over the current task selection. One job → the full
    /// single-row menu (reveal/copy endpoints + cancel/retry/remove); many → bulk
    /// actions that fold over the selection (retry failed, cancel running, remove
    /// finished, copy commands). Jobs are snapshotted live so the menu reflects
    /// state at open time even though it's described declaratively.
    pub(crate) fn task_menu_spec(&self, ids: Vec<usize>, cx: &mut Context<Self>) -> MenuSpec {
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
        spec
    }

    pub(crate) fn bg_menu_spec(&self, cx: &mut Context<Self>) -> MenuSpec {
        let dir_path = self.copy_text(cx);
        let spec = MenuSpec::new()
            .item("bg-new-folder", "New folder", "icons/new_folder.svg", |this, _, cx| this.begin_new_folder(cx))
            .item("bg-upload", "Upload", "icons/upload.svg", |this, _, cx| this.begin_upload(cx))
            .when(self.clipboard.is_some(), |m| {
                m.item("bg-paste", "Paste", "icons/clipboard.svg", |this, _, cx| this.paste_clipboard(cx))
            })
            .item("bg-refresh", "Refresh", "icons/refresh.svg", |this, _, cx| this.force_reload_entries(cx))
            .item("bg-copy-path", "Copy path", "icons/copy.svg", move |this, _, cx| {
                this.copy_to_clipboard(dir_path.clone(), cx)
            });
        spec
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
