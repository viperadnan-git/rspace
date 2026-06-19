//! Transfers panel, settings, and status bar views.

use super::*;

impl Workspace {
    pub(crate) fn render_transfers(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_done = self.jobs.read(cx).has_finished();
        let count = self.jobs.read(cx).items().len();
        let body = if count == 0 {
            // No live transfers: show the persisted history (read-only).
            self.render_job_history(cx)
        } else {
            uniform_list(
                "transfers",
                count,
                cx.processor(|this, range: Range<usize>, _window, cx| {
                    let items = this.jobs.read(cx).items().to_vec();
                    let n = items.len();
                    range
                        // Newest first.
                        .filter_map(|i| {
                            n.checked_sub(1 + i).and_then(|idx| items.get(idx).cloned()).map(|j| (i, j))
                        })
                        .map(|(i, job)| {
                            div()
                                .px_3()
                                .when(i > 0, |d| d.border_t_1().border_color(rgb(BORDER_MUTED)))
                                .child(this.job_row(&job, cx))
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .flex_1()
            .into_any_element()
        };

        let maximized = self.jobs_maximized;
        let outer = if maximized {
            v_flex().flex_1().min_h(px(0.0))
        } else {
            v_flex().h(px(260.0)).flex_shrink_0()
        };
        outer
            .bg(rgb(INSET))
            // Maximized is flush under the title bar's border; only the dock needs its own.
            .when(!maximized, |el| el.border_t_1().border_color(rgb(BORDER_MUTED)))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .px_3()
                    .py_1()
                    .border_b_1()
                    .border_color(rgb(BORDER_MUTED))
                    .child(div().text_color(rgb(FG)).child("Transfers"))
                    .child(
                        h_flex()
                            .gap_1()
                            .when(has_done, |el| {
                                el.child(
                                    icon_button("clear-finished", "icons/trash.svg")
                                        .tooltip(tooltip_text("Clear finished"))
                                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                            this.request_clear_finished(cx)
                                        })),
                                )
                            })
                            .child(
                                icon_button(
                                    "transfers-maximize",
                                    if maximized { "icons/minimize.svg" } else { "icons/maximize.svg" },
                                )
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.jobs_maximized = !this.jobs_maximized;
                                    this.ui.transfers_maximized = this.jobs_maximized;
                                    this.save_ui();
                                    cx.notify();
                                })),
                            )
                            .child(icon_button("transfers-close", "icons/x.svg").on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.jobs_open = false;
                                    cx.notify();
                                }),
                            )),
                    ),
            )
            .child(body)
    }

    /// Read-only history of finished jobs (from the db), shown when no transfers
    /// are live. Empty → the idle placeholder.
    fn render_job_history(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.job_history.is_empty() {
            return centered("No transfers", FG_SUBTLE).into_any_element();
        }
        v_flex()
            .flex_1()
            .min_h(px(0.0))
            .child(section_header("RECENT"))
            .child(
                uniform_list(
                    "transfer-history",
                    self.job_history.len(),
                    cx.processor(|this, range: Range<usize>, _window, _cx| {
                        range.filter_map(|i| this.job_history.get(i).map(job_history_row)).collect::<Vec<_>>()
                    }),
                )
                .flex_1(),
            )
            .into_any_element()
    }

    /// A clickable job endpoint, styled like a breadcrumb crumb: shows the name,
    /// reveals the item in the explorer on click, full `remote:path` on hover.
    fn job_target_chip(
        &self,
        job_id: usize,
        index: usize,
        target: JobTarget,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let full_path = format!("{}:{}", target.remote, target.path);
        let name = target.name.clone();
        div()
            .id(SharedString::from(format!("target-{job_id}-{index}")))
            .min_w(px(0.0))
            .max_w(px(220.0))
            .px_1()
            .rounded_md()
            .truncate()
            .cursor_pointer()
            .text_color(rgb(FG))
            .hover(|s| s.bg(rgba(OVERLAY)))
            .tooltip(tooltip_text(full_path))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.reveal_target(target.clone(), cx)))
            .child(name)
    }

    fn job_row(&self, job: &Job, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let id = job.id;
        let verb = job.verb.clone();
        let targets = job.targets.clone();
        let pct = if job.total > 0 {
            (job.bytes as f64 / job.total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let elapsed = human_duration(job.elapsed_ms);
        let percent = format!("{}%", (pct * 100.0).round() as u32);
        // Title-line status icon: spinner while running, check / alert when settled.
        let icon: AnyElement = if job.error.is_some() {
            svg().path("icons/alert.svg").size(px(14.0)).text_color(rgb(DANGER)).into_any_element()
        } else if job.done {
            svg().path("icons/check.svg").size(px(14.0)).text_color(rgb(SUCCESS)).into_any_element()
        } else {
            spinner(SharedString::from(format!("spin-{id}")), px(14.0), ACCENT).into_any_element()
        };
        // Only meaningful for multi-file transfers; a single file shows just bytes.
        let files = if job.total_transfers > 1 {
            format!("{}/{} files · ", job.transfers, job.total_transfers)
        } else {
            String::new()
        };
        let stats = if job.total > 0 {
            format!(
                "{files}{} / {} · {}/s · {elapsed}",
                human_size(job.bytes as i64),
                human_size(job.total as i64),
                human_size(job.speed as i64)
            )
        } else {
            format!("{files}Starting… · {elapsed}")
        };
        let done_line = if job.total_transfers > 1 {
            format!("Done · {} files · {elapsed}", job.total_transfers)
        } else {
            format!("Done · {elapsed}")
        };

        let command = job.command.clone();
        let error = job.error.clone();
        let action_button = move |suffix: &str, svg_icon: &'static str, tip: &'static str| {
            icon_button(SharedString::from(format!("{suffix}-{id}")), svg_icon).tooltip(tooltip_text(tip))
        };

        v_flex()
            .w_full()
            .py_2()
            .gap_1p5()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(div().flex_shrink_0().child(icon))
                    .child({
                        let mut line = h_flex()
                            .flex_grow(1.0)
                            .min_w(px(0.0))
                            .gap_1()
                            .child(div().flex_shrink_0().text_color(rgb(FG_MUTED)).child(verb));
                        for (index, target) in targets.into_iter().enumerate() {
                            if index > 0 {
                                line = line.child(
                                    div().flex_shrink_0().text_color(rgb(FG_SUBTLE)).child("→"),
                                );
                            }
                            line = line.child(self.job_target_chip(id, index, target, cx));
                        }
                        line
                    })
                    .child(self.copy_button(
                        SharedString::from(format!("copy-cmd-{id}")),
                        CopySource::JobCommand(id),
                        command,
                        "Copy rclone command",
                        cx,
                    ))
                    .when(!job.done, |el| {
                        el.child(action_button("cancel", "icons/x.svg", "Cancel").on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| this.request_cancel_job(id, cx)),
                        ))
                    })
                    .when(job.done && error.is_some(), |el| {
                        el.child(action_button("retry", "icons/refresh.svg", "Retry").on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| this.retry_job(id, cx)),
                        ))
                    })
                    .when(job.done, |el| {
                        el.child(action_button("clear", "icons/trash.svg", "Remove from list").on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| this.clear_job(id, cx)),
                        ))
                    }),
            )
            .when(!job.done, |el| {
                el.child(
                    h_flex()
                        .w_full()
                        .gap_3()
                        .items_center()
                        .child(
                            div()
                                .flex_grow(1.0)
                                .min_w(px(0.0))
                                .truncate()
                                .text_xs()
                                .text_color(rgb(FG_MUTED))
                                .child(stats),
                        )
                        .child(
                            div()
                                .flex_grow(1.0)
                                .min_w(px(140.0))
                                .max_w(px(320.0))
                                .h(px(4.0))
                                .rounded_full()
                                .bg(rgba(OVERLAY))
                                .child(
                                    div().h_full().rounded_full().bg(rgb(ACCENT)).w(relative(pct as f32)),
                                ),
                        )
                        .child(
                            div().w(px(34.0)).flex_shrink_0().text_xs().text_color(rgb(FG_MUTED)).child(percent),
                        ),
                )
            })
            .when(job.done && error.is_none(), |el| {
                el.child(div().text_xs().text_color(rgb(FG_SUBTLE)).child(done_line))
            })
            .when(error.is_some(), |el| {
                el.child(
                    h_flex()
                        .w_full()
                        .gap_3()
                        .items_center()
                        .child(
                            div()
                                .flex_grow(1.0)
                                .min_w(px(0.0))
                                .truncate()
                                .text_xs()
                                .text_color(rgb(DANGER))
                                .child(error.clone().unwrap_or_default()),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(rgb(DANGER))
                                .child(format!("Failed · {elapsed}")),
                        ),
                )
            })
    }

    pub(crate) fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let card = modal_card("settings-card", &self.focus, cx)
            .w(px(480.0))
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
                                this.settings_open = false;
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
                this.settings_open = false;
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

    fn refresh_setting(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        setting_block(
            "Refresh interval",
            "How often open folders revalidate in the background.",
            h_flex()
                .gap_2()
                .items_center()
                .child(self.refresh_field.clone())
                .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child("seconds")),
        )
    }


    fn storage_setting(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (total, clearable) = self.storage_size.unwrap_or_default();
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
        let binary = Some(self.rclone_bin.clone()).filter(|s| !s.is_empty());
        let config = self.rclone_paths.as_ref().map(|p| p.config.clone());
        let cache = self.rclone_paths.as_ref().map(|p| p.cache.clone());
        let cache_label = match self.rclone_cache_size {
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
        let editing = self.rclone_edit.as_ref().filter(|(f, _)| *f == field).map(|(_, i)| i.clone());
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
        self.rclone_edit = Some((field, input));
        self.rclone_edit_focus = true;
        cx.notify();
    }

    fn cancel_rclone_edit(&mut self, cx: &mut Context<Self>) {
        self.rclone_edit = None;
        cx.notify();
    }

    /// Save the edited override (validating the binary), then relaunch.
    fn confirm_rclone_edit(&mut self, cx: &mut Context<Self>) {
        let Some((field, input)) = self.rclone_edit.as_ref() else {
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
        if let Some((_, input)) = self.rclone_edit.as_ref() {
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
        if self.rclone_paths.is_none() {
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
        let Some(cache) = self.rclone_paths.as_ref().map(|p| p.cache.clone()) else {
            return;
        };
        let _ = std::fs::remove_dir_all(&cache);
        self.rclone_cache_size = Some(0);
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
        self.db.clear_history();
        delete_rotated_logs(&self.paths.logs_dir());
        self.recent_remotes.clear();
        self.job_history.clear();
        self.refresh_storage_size();
        self.toast("Cleaned up", false, cx);
        cx.notify();
    }

    fn settings_info(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let root = self.paths.root().display().to_string();
        v_flex()
            .gap_3()
            .pt_3()
            .border_t_1()
            .border_color(rgb(BORDER_MUTED))
            .child(info_row("rclone", &self.version))
            .child(self.path_link("data-folder", Some("Data folder".into()), Some(root), cx))
    }

    pub(crate) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let info = if self.open_remote.is_some() {
            if self.selected.len() > 1 {
                format!("{} selected", self.selected.len())
            } else {
                format!("{} items", self.entries().len())
            }
        } else {
            format!("{} remotes", self.remotes.len())
        };
        h_flex()
            .w_full()
            .flex_shrink_0()
            .justify_between()
            // Left holds the daemon icon button — tighten so it hugs the corner
            // (Zed-style), matching the vertical inset; keep the right text padded.
            .pl_1()
            .pr_3()
            .py_1()
            .border_t_1()
            .border_color(rgb(BORDER_MUTED))
            .bg(rgb(INSET))
            .text_xs()
            .text_color(rgb(FG_MUTED))
            .child(
                h_flex().gap_2().child(self.rc_status(cx)).children(self.active_remote().map(|r| {
                    h_flex()
                        .gap_2()
                        .child(div().text_color(rgb(FG)).child(r.name.clone()))
                        .child(div().text_color(rgb(FG_SUBTLE)).child(r.kind.clone()))
                })),
            )
            .child(
                h_flex()
                    .gap_3()
                    .when(!self.jobs.read(cx).is_empty(), |el| el.child(self.jobs_indicator(cx)))
                    .child(info)
                    .child(self.version.clone()),
            )
    }

    /// Status-bar daemon button: an icon whose color tracks health (red on
    /// error), opening the rcd popover anchored to this button. The tooltip is
    /// suppressed while the popover is open, like Zed's status-bar buttons.
    fn rc_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.rc_popover_open;
        // Static cases stay zero-alloc; only the rare `Down` message formats.
        let (color, tip): (u32, SharedString) = match &self.rc_health {
            RcHealth::Up => (FG_MUTED, "rclone rc daemon connected".into()),
            RcHealth::Down(e) => (DANGER, format!("rclone rc daemon unreachable: {e}").into()),
            RcHealth::Restarting => (FG_MUTED, "Restarting rclone daemon…".into()),
            RcHealth::Unknown => (FG_SUBTLE, "Checking rclone daemon…".into()),
        };
        let icon: AnyElement = if matches!(self.rc_health, RcHealth::Restarting) {
            spinner("rc-spin", px(15.0), FG_MUTED).into_any_element()
        } else {
            svg().path(self.rc_health.icon()).size(px(15.0)).flex_shrink_0().text_color(rgb(color)).into_any_element()
        };
        div()
            .relative()
            .child(
                h_flex()
                    .id("rc-status")
                    .p(px(3.0))
                    .items_center()
                    .rounded_md()
                    .cursor_pointer()
                    .when(open, |el| el.bg(rgba(OVERLAY)))
                    .hover(|s| s.bg(rgba(OVERLAY)))
                    .child(icon)
                    .when(!open, |el| el.tooltip(tooltip_text(tip)))
                    // Only reachable while closed — the open backdrop intercepts clicks.
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.close_menus();
                        this.rc_popover_open = true;
                        cx.notify();
                    })),
            )
            .when(open, |el| {
                el.child(
                    deferred(
                        div().absolute().bottom_full().left_0().pb_1().child(self.rc_popover_card(cx)),
                    )
                    .priority(3),
                )
            })
    }

    /// Full-window click-catcher that dismisses the open rcd popover; rendered at
    /// the workspace root, below the popover card. Avoids the trigger/`mouse_down_out`
    /// double-fire by intercepting the next mouse-down anywhere outside the card.
    pub(crate) fn rc_popover_backdrop(&self, cx: &mut Context<Self>) -> impl IntoElement {
        deferred(
            div().absolute().top_0().left_0().size_full().occlude().on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.close_menus();
                    cx.notify();
                }),
            ),
        )
        .priority(2)
    }

    /// The daemon status + actions card shown by [`rc_status`].
    fn rc_popover_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (color, status) = match &self.rc_health {
            RcHealth::Unknown => (FG_SUBTLE, "Connecting…"),
            RcHealth::Up => (SUCCESS, "Connected"),
            RcHealth::Down(_) => (DANGER, "Disconnected"),
            RcHealth::Restarting => (FG_MUTED, "Restarting…"),
        };
        let subtitle = match (&self.rc_health, self.version.is_empty()) {
            (RcHealth::Up, false) => format!("{status} · rclone {}", self.version),
            _ => status.to_string(),
        };
        let logs = self.paths.logs_dir().to_string_lossy().into_owned();
        let mut items: Vec<AnyElement> = Vec::new();
        items.push(
            v_flex()
                .w_full()
                .px_2()
                .py_1()
                .gap(px(2.0))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(svg().path(self.rc_health.icon()).size(px(14.0)).flex_shrink_0().text_color(rgb(color)))
                        .child(div().text_color(rgb(FG)).child("rclone daemon")),
                )
                .child(div().text_xs().text_color(rgb(FG_MUTED)).child(subtitle))
                .into_any_element(),
        );
        if let RcHealth::Down(e) = &self.rc_health {
            items.push(
                div().w_full().px_2().pb_1().text_xs().text_color(rgb(DANGER)).child(e.clone()).into_any_element(),
            );
        }
        items.push(div().w_full().my_1().h(px(1.0)).bg(rgb(BORDER_MUTED)).into_any_element());
        items.push(
            self.menu_item("Reconnect", "icons/activity.svg", cx, |this, cx| this.reconnect_daemon(cx))
                .into_any_element(),
        );
        // Restarting already in flight: skip a redundant restart.
        if !matches!(self.rc_health, RcHealth::Restarting) {
            items.push(
                self.menu_item("Restart daemon", "icons/refresh.svg", cx, |this, cx| this.restart_daemon(cx))
                    .into_any_element(),
            );
        }
        items.push(
            self.menu_item("Copy logs path", "icons/copy.svg", cx, move |this, cx| {
                this.copy_to_clipboard(logs.clone(), cx)
            })
            .into_any_element(),
        );
        self.popover_surface("rc-popover", items, cx).w(px(220.0))
    }

    /// Mark the daemon healthy and re-sync the views (after a reconnect/restart).
    fn on_daemon_up(&mut self, cx: &mut Context<Self>) {
        self.rc_health = RcHealth::Up;
        self.load_remotes(cx);
        if self.open_remote.is_some() {
            self.force_reload_entries(cx);
        }
    }

    /// Re-ping the daemon and refresh the listings (recover a lost connection).
    fn reconnect_daemon(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        cx.spawn(async move |this, cx| {
            let result = service.ping().await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.on_daemon_up(cx),
                    Err(e) => this.rc_health = RcHealth::Down(e.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Stop and respawn `rcd`, then refresh. The swap-able client means every
    /// in-flight and future call picks up the new endpoint automatically.
    pub(crate) fn restart_daemon(&mut self, cx: &mut Context<Self>) {
        self.rc_health = RcHealth::Restarting;
        let service = self.service.clone();
        cx.spawn(async move |this, cx| {
            let result = service.restart_daemon().await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.on_daemon_up(cx),
                    Err(e) => this.rc_health = RcHealth::Down(e.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn jobs_indicator(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Separate counts so a mixed run reads e.g. "↻2  ✓3  ⚠1".
        let jobs = self.jobs.read(cx);
        let active = jobs.items().iter().filter(|j| !j.done).count();
        let failed = jobs.items().iter().filter(|j| j.done && j.error.is_some()).count();
        let succeeded = jobs.items().iter().filter(|j| j.done && j.error.is_none()).count();
        h_flex()
            .id("jobs-indicator")
            .gap_2()
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .hover(|s| s.bg(rgba(OVERLAY)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.jobs_open = !this.jobs_open;
                cx.notify();
            }))
            .when(active > 0, |el| {
                el.child(
                    h_flex()
                        .gap_1()
                        .text_color(rgb(ACCENT))
                        .child(spinner_icon("jobs-active-spin", "icons/refresh.svg", px(13.0), ACCENT))
                        .child(active.to_string()),
                )
            })
            .when(succeeded > 0, |el| el.child(count_badge("icons/check.svg", SUCCESS, succeeded)))
            .when(failed > 0, |el| el.child(count_badge("icons/alert.svg", DANGER, failed)))
    }
}

/// A read-only finished-job row for the transfers history list.
fn job_history_row(job: &JobRecord) -> Div {
    let path = match (&job.source, &job.dest) {
        (Some(s), Some(d)) => format!("{s} \u{2192} {d}"),
        (Some(s), _) => s.clone(),
        _ => String::new(),
    };
    let meta = if job.bytes > 0 {
        format!("{} · {}", human_size(job.bytes), relative_time(job.finished_at))
    } else {
        relative_time(job.finished_at)
    };
    let ok = job.ok;
    h_flex()
        .w_full()
        .gap_2()
        .px_3()
        .py_1()
        .items_center()
        .border_t_1()
        .border_color(rgb(SEPARATOR))
        .child(div().size(px(6.0)).flex_shrink_0().rounded_full().bg(rgb(if ok { SUCCESS } else { DANGER })))
        .child(div().flex_shrink_0().text_color(rgb(FG)).child(job.op.clone()))
        .child(div().flex_1().min_w(px(0.0)).truncate().text_xs().text_color(rgb(FG_MUTED)).child(path))
        .child(div().flex_shrink_0().text_xs().text_color(rgb(FG_SUBTLE)).child(meta))
}

/// Coarse "time ago" label for a unix-epoch-seconds timestamp.
fn relative_time(epoch_secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(epoch_secs);
    match (now - epoch_secs).max(0) {
        0..=59 => "just now".into(),
        s @ 60..=3599 => format!("{}m ago", s / 60),
        s @ 3600..=86_399 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

/// Delete rotated log files, keeping the active (most-recently-modified) one:
/// unlinking the open file would lose writes to its now-detached inode.
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
