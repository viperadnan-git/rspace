//! Settings panel, command palette, dialogs, daemon.

use super::*;

impl Workspace {
    pub(crate) fn toggle_palette(&mut self, _: &TogglePalette, window: &mut Window, cx: &mut Context<Self>) {
        if self.modal_is::<Picker<CommandPaletteDelegate>>() {
            self.close_modal(cx);
            return;
        }
        if self.modal.is_some() || self.prompt.is_some() {
            return;
        }
        let previous_focus = window.focused(cx).unwrap_or_else(|| self.focus.clone());
        let workspace = cx.entity().downgrade();
        let service = self.app.service.clone();
        let db = self.app.db.clone();
        // Pinned-first (pin order preserved), matching the sidebar; the palette's
        // stable fuzzy sort keeps this order on empty query and score ties.
        let remotes = self.ordered_remotes();
        let current_remote = self.open_remote.clone();
        let palette = cx.new(|cx| {
            let delegate = CommandPaletteDelegate::new(
                previous_focus,
                workspace,
                service,
                db,
                remotes,
                current_remote,
                window,
            );
            Picker::new(delegate, window, cx)
        });
        let sub = cx.subscribe(&palette, |this, _, _: &DismissEvent, cx| this.close_modal(cx));
        self.show_modal(ActiveModal::new(palette).align_top().deferred().subscribe(sub), cx);
    }

    pub(crate) fn action_add_remote(&mut self, _: &AddRemote, _: &mut Window, cx: &mut Context<Self>) {
        self.begin_add_remote(cx);
    }

    pub(crate) fn action_open_settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.open_settings(cx);
    }

    pub(crate) fn action_show_keybindings(&mut self, _: &ShowKeybindings, _: &mut Window, cx: &mut Context<Self>) {
        self.open_keybindings(cx);
    }

    pub(crate) fn open_keybindings(&mut self, cx: &mut Context<Self>) {
        let view = cx.new(KeybindingsView::new);
        let sub = cx.subscribe(&view, |this, _, _: &DismissEvent, cx| this.close_modal(cx));
        self.show_modal(ActiveModal::new(view).subscribe(sub), cx);
    }

    pub(crate) fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings.open = true;
        // Sync the font-size field to the live value (also changed by the zoom keys).
        let font_px = self.ui_font_size().round() as u64;
        self.settings.ui_font_field.update(cx, |f, cx| f.set_value(font_px, cx));
        self.refresh_storage_size();
        self.fetch_rclone_info(cx);
        cx.notify();
    }

    pub(crate) fn refresh_storage_size(&mut self) {
        self.settings.storage_size = Some((dir_size(self.app.paths.root()), dir_size(&self.app.paths.cache_dir())));
    }

    /// Resolve rclone's own paths (`config/paths`, fetched once) and size its
    /// cache. The VFS cache can be many GB, so the walk runs on the background
    /// executor rather than blocking the UI thread.
    pub(crate) fn fetch_rclone_info(&mut self, cx: &mut Context<Self>) {
        let service = self.app.service.clone();
        // Resolve paths only once (they don't change at runtime); the size walk
        // runs every open so it stays fresh.
        let cache = self.settings.rclone_paths.as_ref().map(|p| p.cache.clone());
        cx.spawn(async move |this, cx| {
            let (cache, fetched) = match cache {
                Some(cache) => (cache, None),
                None => match service.config_paths().await {
                    Ok(paths) => (paths.cache.clone(), Some(paths)),
                    Err(_) => return,
                },
            };
            let size = cx.background_executor().spawn(async move { dir_size(Path::new(&cache)) }).await;
            this.update(cx, |this, cx| {
                this.settings.rclone_cache_size = Some(size);
                if let Some(paths) = fetched {
                    this.settings.rclone_paths = Some(paths);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn action_restart_daemon(&mut self, _: &RestartDaemon, _: &mut Window, cx: &mut Context<Self>) {
        self.daemon.update(cx, |d, cx| d.restart(cx));
    }

    pub(crate) fn action_toggle_tasks(&mut self, _: &ToggleTasks, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_dock(DockPanel::Tasks, cx);
    }

    pub(crate) fn ask_confirm(
        &mut self,
        title: impl Into<SharedString>,
        message: impl Into<SharedString>,
        confirm_label: impl Into<SharedString>,
        danger: bool,
        action: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let modal =
            cx.new(|cx| ConfirmModal::new(title, message, confirm_label, danger, cx));
        let mut action = Some(action);
        let sub = cx.subscribe(&modal, move |this, _, event, cx| {
            this.modal = None;
            if let confirm::ConfirmEvent::Accepted = event {
                if let Some(action) = action.take() {
                    action(this, cx);
                }
            }
            cx.notify();
        });
        self.show_modal(ActiveModal::new(modal).deferred().subscribe(sub), cx);
    }

    /// Start an inline edit; `action` runs with the entered text on submit.
    /// `target` is the renamed entry's path, or `None` for a new item at the top.
    pub(crate) fn begin_edit(
        &mut self,
        value: impl Into<String>,
        placeholder: impl Into<SharedString>,
        icon_dir: bool,
        target: Option<String>,
        action: impl FnOnce(&mut Self, String, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let modal =
            cx.new(|cx| PromptModal::new(value, placeholder, icon_dir, target, cx));
        let mut action = Some(action);
        self.prompt_sub = Some(cx.subscribe(&modal, move |this, _, event, cx| {
            match event {
                prompt::PromptEvent::Submitted(value) => {
                    this.prompt = None;
                    if let Some(action) = action.take() {
                        action(this, value.clone(), cx);
                    }
                }
                prompt::PromptEvent::Cancelled => this.prompt = None,
            }
            cx.notify();
        }));
        self.prompt = Some(modal);
        cx.notify();
    }

    pub(crate) fn close_settings(&mut self, _: &CloseSettings, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.open
            || self.menus.context.is_some()
            || self.menus.remote_menu.is_some()
            || self.menus.bg_menu.is_some()
            || self.modal.is_some()
            || self.prompt.is_some()
            || self.dock_is(DockPanel::Tasks)
        {
            self.settings.open = false;
            // Esc dismisses the transient Tasks dock, but leaves the Preview be.
            if self.dock_is(DockPanel::Tasks) {
                self.dock = None;
            }
            self.prompt = None;
            self.close_modal(cx);
            self.close_menus();
            cx.notify();
        } else if self.explorer_focused(window, cx) && self.explorer.read(cx).selection_len() > 1 {
            // Nothing to close: collapse a multi-selection back to the cursor.
            self.explorer.update(cx, |e, cx| e.collapse_selection(cx));
            cx.notify();
        }
    }

    pub(crate) fn set_refresh(&mut self, secs: u64, cx: &mut Context<Self>) {
        self.store.update(|s| s.refresh_secs = secs);
        self.explorer.update(cx, |e, _| e.set_refresh(secs));
        cx.notify();
    }

    pub(crate) fn choose_download_dir(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(dir) = paths.into_iter().next() {
                    this.update(cx, |this, cx| {
                        this.store.update(|s| s.download_dir = Some(dir.to_string_lossy().into_owned()));
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

}
