//! Right-click context menus and the shared popover surface.

use super::*;

impl Workspace {
    fn menu_item(
        &self,
        label: &'static str,
        icon: &'static str,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        self.menu_item_toned(label, icon, FG, FG_MUTED, cx, action)
    }

    /// A destructive menu item, tinted with the danger color.
    fn menu_item_danger(
        &self,
        label: &'static str,
        icon: &'static str,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
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
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
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
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                action(this, cx);
                this.close_menus();
                cx.notify();
            }))
            .child(svg().path(icon).size(px(15.0)).flex_shrink_0().text_color(rgb(icon_color)))
            .child(label)
    }

    /// Close every transient popover.
    pub(crate) fn close_menus(&mut self) {
        self.context = None;
        self.remote_menu = None;
        self.bg_menu = None;
    }

    /// Popover anchored at `pos`, dismissed on outside mouse-down. `occlude`
    /// stops hover/click reaching content behind it.
    fn popover(
        &self,
        id: &'static str,
        pos: Point<Pixels>,
        items: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let menu = v_flex()
            .id(id)
            .occlude()
            .min_w(px(180.0))
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
            .children(items);
        deferred(anchored().position(pos).snap_to_window_with_margin(px(8.0)).child(menu)).priority(2)
    }

    pub(crate) fn render_context_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (entry, pos) = self.context.clone().unwrap();
        let remote = self.open_remote.clone().unwrap_or_default();
        let mut items: Vec<AnyElement> = Vec::new();

        if entry.is_dir {
            let (e, r) = (entry.clone(), remote.clone());
            items.push(
                self.menu_item("Open", "icons/folder_open.svg", cx, move |this, cx| {
                    this.navigate(r.clone(), e.path.clone(), None, cx)
                })
                .into_any_element(),
            );
        }
        let (e_cp, r_cp) = (entry.clone(), remote.clone());
        items.push(
            self.menu_item("Download", "icons/download.svg", cx, move |this, cx| {
                this.download_selected(cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item("Copy path", "icons/copy.svg", cx, move |this, cx| {
                this.copy_to_clipboard(format!("{}:{}", r_cp, e_cp.path), cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item("Copy", "icons/copy.svg", cx, move |this, cx| {
                this.set_clipboard(TransferMode::Copy, cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item("Cut", "icons/scissors.svg", cx, move |this, cx| {
                this.set_clipboard(TransferMode::Move, cx)
            })
            .into_any_element(),
        );
        if self.clipboard.is_some() {
            items.push(
                self.menu_item("Paste", "icons/clipboard.svg", cx, move |this, cx| {
                    this.paste_clipboard(cx)
                })
                .into_any_element(),
            );
        }
        let (e_rn, r_rn) = (entry.clone(), remote.clone());
        items.push(
            self.menu_item("Rename", "icons/edit.svg", cx, move |this, cx| {
                this.begin_rename(r_rn.clone(), e_rn.clone(), cx)
            })
            .into_any_element(),
        );
        items.push(
            self.menu_item_danger("Delete", "icons/trash.svg", cx, |this, cx| {
                this.request_delete_selected(cx)
            })
            .into_any_element(),
        );

        self.popover("context-menu", pos, items, cx)
    }

    pub(crate) fn render_remote_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (name, pos) = self.remote_menu.clone().unwrap();
        let pinned = self.is_pinned(&name);
        let mut items: Vec<AnyElement> = Vec::new();

        let open_name = name.clone();
        items.push(
            self.menu_item("Open", "icons/folder_open.svg", cx, move |this, cx| {
                if let Some(ix) = this.ordered_remotes().iter().position(|r| r.name == open_name) {
                    this.load_remote(ix, cx);
                }
            })
            .into_any_element(),
        );

        let pin_name = name.clone();
        let (pin_label, pin_icon) = if pinned { ("Unpin", "icons/pin.svg") } else { ("Pin", "icons/pin.svg") };
        items.push(
            self.menu_item(pin_label, pin_icon, cx, move |this, cx| {
                this.toggle_pin(pin_name.clone(), cx)
            })
            .into_any_element(),
        );

        if pinned {
            let up_name = name.clone();
            let down_name = name.clone();
            items.push(
                self.menu_item("Move up", "icons/chevron_up.svg", cx, move |this, cx| {
                    this.move_pinned(&up_name, true, cx)
                })
                .into_any_element(),
            );
            items.push(
                self.menu_item("Move down", "icons/chevron_down.svg", cx, move |this, cx| {
                    this.move_pinned(&down_name, false, cx)
                })
                .into_any_element(),
            );
        }

        self.popover("remote-menu", pos, items, cx)
    }

    pub(crate) fn render_bg_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let pos = self.bg_menu.unwrap();
        let mut items: Vec<AnyElement> = Vec::new();
        items.push(
            self.menu_item("New folder", "icons/folder.svg", cx, |this, cx| this.begin_new_folder(cx))
                .into_any_element(),
        );
        items.push(
            self.menu_item("Upload", "icons/upload.svg", cx, |this, cx| this.begin_upload(cx))
                .into_any_element(),
        );
        if self.clipboard.is_some() {
            items.push(
                self.menu_item("Paste", "icons/clipboard.svg", cx, |this, cx| this.paste_clipboard(cx))
                    .into_any_element(),
            );
        }
        items.push(
            self.menu_item("Refresh", "icons/refresh.svg", cx, |this, cx| this.force_reload_entries(cx))
                .into_any_element(),
        );
        let dir_path = self.copy_text();
        items.push(
            self.menu_item("Copy path", "icons/copy.svg", cx, move |this, cx| {
                this.copy_to_clipboard(dir_path.clone(), cx)
            })
            .into_any_element(),
        );
        self.popover("bg-menu", pos, items, cx)
    }
}
