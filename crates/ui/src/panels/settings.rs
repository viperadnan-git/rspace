//! The settings panel: download dir, refresh, rclone paths, storage.

use super::*;

impl Workspace {
    pub(crate) fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let card = modal_card("settings-card", &self.focus, cx)
            .w(rem(480.0))
            .max_h(relative(0.85))
            .gap_0()
            .child(
                h_flex()
                    .flex_shrink_0()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .pb_4()
                    .border_b_1()
                    .border_color(rgb(BORDER_MUTED))
                    .child(div().text_lg().text_color(rgb(FG)).child("Settings"))
                    .child(
                        icon_button("settings-close", "icons/x.svg").on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.settings.open = false;
                                cx.notify();
                            },
                        )),
                    ),
            )
            .child(
                v_flex()
                    .id("settings-body")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_scroll()
                    .pt_4()
                    .gap_5()
                    .child(self.refresh_setting(cx))
                    .child(self.ui_font_setting(cx))
                    .child(self.path_bar_setting(cx))
                    .child(divider())
                    .child(self.download_setting(cx))
                    .child(divider())
                    .child(self.storage_setting(cx))
                    .child(divider())
                    .child(self.rclone_setting(cx))
                    .child(self.settings_info(cx)),
            );
        self.modal_overlay(
            true,
            false,
            |this, cx| {
                this.settings.open = false;
                cx.notify();
            },
            card,
            cx,
        )
    }

    fn download_setting(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.store.get().download_dir().display().to_string();
        setting_block(
            "Download location",
            "Where files are saved. Defaults to your Downloads folder.",
            h_flex()
                .gap_2()
                .items_center()
                .child(div().flex_grow(1.0).min_w(px(0.0)).child(self.path_link(
                    "download-dir",
                    None,
                    Some(current),
                    cx,
                )))
                .child(Button::new("choose-dir", "Choose…", ButtonStyle::Soft).build(|this, cx| {
                    this.choose_download_dir(cx)
                }, cx)),
        )
    }

    fn path_bar_setting(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let on = self.store.get().show_path_bar;
        setting_block(
            "Path bar",
            "Show the breadcrumb path at the bottom of the explorer.",
            switch("toggle-path-bar", on, None, |this, cx| {
                this.store.update(|s| s.show_path_bar = !s.show_path_bar);
                cx.notify();
            }, cx),
        )
    }

    fn refresh_setting(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        setting_block(
            "Refresh interval",
            "How often open folders revalidate in the background.",
            h_flex()
                .gap_2()
                .items_center()
                .child(self.settings.refresh_field.clone())
                .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child("seconds")),
        )
    }

    fn ui_font_setting(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        setting_block(
            "UI font size",
            "Base interface font size; everything scales from it. Also \u{2318}+ / \u{2318}- / \u{2318}0.",
            h_flex()
                .gap_2()
                .items_center()
                .child(self.settings.ui_font_field.clone())
                .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child("px")),
        )
    }


    fn storage_setting(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (total, clearable) = self.settings.storage_size.unwrap_or_default();
        let summary = format!("{} · {} clearable", human_size(total as i64), human_size(clearable as i64));
        setting_block(
            "Storage",
            "History and logs the app keeps on disk. Clean up clears these; your preferences and pinned remotes are kept.",
            h_flex()
                .gap_2()
                .items_center()
                .child(div().flex_grow(1.0).min_w(px(0.0)).truncate().text_xs().text_color(rgb(FG_MUTED)).child(summary))
                .child(Button::new("clean-up", "Clean up", ButtonStyle::Soft).build(|this, cx| this.request_cleanup(cx), cx)),
        )
    }

    /// rclone's own paths (from `config/paths`, so correct per-OS) plus its cache
    /// size and a clear action. Paths are clickable and open with the OS default.
    fn rclone_setting(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let binary = Some(self.settings.rclone_bin.clone()).filter(|s| !s.is_empty());
        let config = self.settings.rclone_paths.as_ref().map(|p| p.config.clone());
        let cache = self.settings.rclone_paths.as_ref().map(|p| p.cache.clone());
        let cache_label = match self.settings.rclone_cache_size {
            Some(b) => SharedString::from(format!("Cache · {}", human_size(b as i64))),
            None => "Cache".into(),
        };
        let bin_override = self.store.get().rclone_path.is_some();
        let config_override = self.store.get().rclone_config_path.is_some();
        setting_block(
            "rclone",
            "The binary, config, and cache rspace uses. Auto-resolved unless you override them; changing relaunches the app.",
            v_flex()
                .gap_3()
                .child(self.rclone_path_row(RcloneField::Binary, "Binary", binary, bin_override, cx))
                .child(self.rclone_path_row(RcloneField::Config, "Config file", config, config_override, cx))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().flex_grow(1.0).min_w(px(0.0)).child(self.path_link(
                            "rclone-cache",
                            Some(cache_label),
                            cache,
                            cx,
                        )))
                        .child(Button::new("clear-rclone-cache", "Clear", ButtonStyle::Soft).build(|this, cx| {
                            this.request_clear_rclone_cache(cx)
                        }, cx)),
                ),
        )
    }

    /// One rclone override row. Normally a path link + Change (+ Reset when
    /// overridden); while editing, a compact inline input + Browse/Cancel/Save so
    /// the path can be typed or picked.
    fn rclone_path_row(
        &self,
        field: RcloneField,
        label: &'static str,
        current: Option<String>,
        overridden: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (link_id, change_id, reset_id, browse_id, cancel_id, save_id) = match field {
            RcloneField::Binary => {
                ("rclone-bin", "change-bin", "reset-bin", "browse-bin", "cancel-bin", "save-bin")
            }
            RcloneField::Config => {
                ("rclone-config", "change-config", "reset-config", "browse-config", "cancel-config", "save-config")
            }
        };
        let editing = self.settings.rclone_edit.as_ref().filter(|(f, _)| *f == field).map(|(_, i)| i.clone());
        if let Some(input) = editing {
            return h_flex()
                .w_full()
                .gap_1()
                .items_center()
                .child(div().flex_1().min_w(px(0.0)).child(input))
                .child(Button::new(browse_id, "Browse", ButtonStyle::Soft).build(|this, cx| this.browse_rclone_edit(cx), cx))
                .child(Button::new(cancel_id, "Cancel", ButtonStyle::Secondary).build(|this, cx| this.cancel_rclone_edit(cx), cx))
                .child(Button::new(save_id, "Save", ButtonStyle::Primary).build(|this, cx| this.confirm_rclone_edit(cx), cx))
                .into_any_element();
        }
        let current_for_edit = current.clone().unwrap_or_default();
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(div().flex_grow(1.0).min_w(px(0.0)).child(self.path_link(link_id, Some(label.into()), current, cx)))
            .child(
                h_flex()
                    .gap_1()
                    .child(Button::new(change_id, "Change", ButtonStyle::Soft).build(move |this, cx| {
                        this.begin_rclone_edit(field, current_for_edit.clone(), cx)
                    }, cx))
                    .when(overridden, |el| {
                        el.child(
                            Button::new(reset_id, "Reset", ButtonStyle::Secondary)
                                .build(move |this, cx| this.reset_rclone(field, cx), cx)
                                .tooltip(tooltip_text("Use automatic resolution")),
                        )
                    }),
            )
            .into_any_element()
    }

    fn begin_rclone_edit(&mut self, field: RcloneField, current: String, cx: &mut Context<Self>) {
        let input = cx.new(|cx| crate::text_input::TextInput::new(cx, field.placeholder()));
        if !current.is_empty() {
            input.update(cx, |i, cx| i.set_text(current, cx));
        }
        self.settings.rclone_edit = Some((field, input));
        self.settings.rclone_edit_focus = true;
        cx.notify();
    }

    fn cancel_rclone_edit(&mut self, cx: &mut Context<Self>) {
        self.settings.rclone_edit = None;
        cx.notify();
    }

    /// Save the edited override (validating the binary), then relaunch.
    fn confirm_rclone_edit(&mut self, cx: &mut Context<Self>) {
        let Some((field, input)) = self.settings.rclone_edit.as_ref() else {
            return;
        };
        let field = *field;
        let path = input.read(cx).text().trim().to_string();
        if path.is_empty() {
            self.cancel_rclone_edit(cx);
            return;
        }
        if field == RcloneField::Binary && rspace_rclone_rc::from_path(&path).is_none() {
            self.toast("That isn't a working rclone binary", true, cx);
            return;
        }
        self.set_rclone_override(field, Some(path));
        relaunch();
    }

    /// Fill the edit input from a file picker (the user still confirms).
    fn browse_rclone_edit(&mut self, cx: &mut Context<Self>) {
        if let Some((_, input)) = self.settings.rclone_edit.as_ref() {
            pick_file_into(input.clone(), cx);
        }
    }

    /// Drop an override and relaunch (back to automatic resolution).
    fn reset_rclone(&mut self, field: RcloneField, _cx: &mut Context<Self>) {
        self.set_rclone_override(field, None);
        relaunch();
    }

    /// Persist (`Some`) or clear (`None`) the override for `field`.
    fn set_rclone_override(&mut self, field: RcloneField, value: Option<String>) {
        self.store.update(|s| match field {
            RcloneField::Binary => s.rclone_path = value,
            RcloneField::Config => s.rclone_config_path = value,
        });
    }

    /// A labeled, truncating path that opens with the OS default app on click
    /// (highlights on hover). `w_full` gives `truncate` a bounded width to clip
    /// against, so it shows the path rather than just an ellipsis.
    fn path_link(
        &self,
        id: &'static str,
        label: Option<SharedString>,
        path: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let display: SharedString = path.clone().map(Into::into).unwrap_or_else(|| "…".into());
        let mut text = div()
            .id(id)
            .w_full()
            .truncate()
            .text_xs()
            .text_color(rgb(FG_MUTED))
            .child(display);
        if let Some(path) = path {
            text = text
                .cursor_pointer()
                .tooltip(tooltip_text("Open with default app"))
                .hover(|s| s.text_color(rgb(ACCENT)))
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.open_with_system(Path::new(&path))
                }));
        }
        v_flex()
            .w_full()
            .min_w(px(0.0))
            .gap(px(2.0))
            .children(label.map(|l| div().text_xs().text_color(rgb(FG_SUBTLE)).child(l)))
            .child(text)
    }

    /// Refuse while mounts are live (clearing the cache would corrupt them),
    /// else confirm before deleting rclone's cache dir.
    fn request_clear_rclone_cache(&mut self, cx: &mut Context<Self>) {
        if !self.mounted.is_empty() {
            self.toast("Unmount all remotes before clearing the cache", true, cx);
            return;
        }
        if self.settings.rclone_paths.is_none() {
            return;
        }
        self.ask_confirm(
            "Clear rclone cache?",
            "Deletes rclone's shared cache directory (used by all rclone tools on this system). It is rebuilt on demand.",
            "Clear",
            false,
            |this, cx| this.clear_rclone_cache(cx),
            cx,
        );
    }

    fn clear_rclone_cache(&mut self, cx: &mut Context<Self>) {
        if !self.mounted.is_empty() {
            self.toast("Unmount all remotes before clearing the cache", true, cx);
            return;
        }
        let Some(cache) = self.settings.rclone_paths.as_ref().map(|p| p.cache.clone()) else {
            return;
        };
        let _ = std::fs::remove_dir_all(&cache);
        self.settings.rclone_cache_size = Some(0);
        self.toast("rclone cache cleared", false, cx);
    }

    /// Confirm, then clear disposable history + logs (keeps preferences + pins).
    fn request_cleanup(&mut self, cx: &mut Context<Self>) {
        self.ask_confirm(
            "Clean up data?",
            "Clears recent remotes, command history, the job log, and old logs. Your preferences and pinned remotes are kept.",
            "Clean up",
            false,
            |this, cx| this.cleanup_storage(cx),
            cx,
        );
    }

    fn cleanup_storage(&mut self, cx: &mut Context<Self>) {
        self.app.db.clear_history();
        delete_rotated_logs(&self.app.paths.logs_dir());
        self.recent_remotes.clear();
        self.refresh_storage_size();
        self.toast("Cleaned up", false, cx);
        cx.notify();
    }

    fn settings_info(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let root = self.app.paths.root().display().to_string();
        v_flex()
            .gap_3()
            .pt_3()
            .border_t_1()
            .border_color(rgb(BORDER_MUTED))
            .child(info_row("rclone", &self.version))
            .child(self.path_link("data-folder", Some("Data folder".into()), Some(root), cx))
    }
}

fn delete_rotated_logs(logs_dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(logs_dir) else {
        return;
    };
    let files: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).collect();
    let active = files
        .iter()
        .filter_map(|p| p.metadata().ok()?.modified().ok().map(|t| (p, t)))
        .max_by_key(|&(_, t)| t)
        .map(|(p, _)| p.clone());
    for p in &files {
        if Some(p) != active.as_ref() {
            let _ = std::fs::remove_file(p);
        }
    }
}
