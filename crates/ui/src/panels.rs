//! Transfers panel, settings, and status bar views.

use super::*;

impl Workspace {
    pub(crate) fn render_transfers(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_done = self.jobs.iter().any(|j| j.done);
        let count = self.jobs.len();
        let body = if count == 0 {
            centered("No transfers", FG_SUBTLE).into_any_element()
        } else {
            uniform_list(
                "transfers",
                count,
                cx.processor(|this, range: Range<usize>, _window, cx| {
                    let n = this.jobs.len();
                    range
                        // Newest first.
                        .filter_map(|i| {
                            n.checked_sub(1 + i).and_then(|idx| this.jobs.get(idx).cloned()).map(|j| (i, j))
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
                                    this.store.update(|s| s.transfers_maximized = this.jobs_maximized);
                                    cx.notify();
                                })),
                            )
                            .child(
                                h_flex()
                                    .id("transfers-close")
                                    .size(px(22.0))
                                    .justify_center()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .text_color(rgb(FG_MUTED))
                                    .hover(|s| s.bg(rgba(OVERLAY)))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.jobs_open = false;
                                        cx.notify();
                                    }))
                                    .child("✕"),
                            ),
                    ),
            )
            .child(body)
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
        let card = self
            .modal_card("settings-card", cx)
            .w(px(460.0))
            .gap_5()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
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
            .child(self.refresh_setting(cx))
            .child(self.download_setting(cx))
            .child(self.settings_info());
        self.modal_overlay(
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
                .child(
                    div()
                        .flex_grow(1.0)
                        .min_w(px(0.0))
                        .truncate()
                        .text_xs()
                        .text_color(rgb(FG_MUTED))
                        .child(current),
                )
                .child(
                    h_flex()
                        .id("choose-dir")
                        .flex_shrink_0()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .bg(rgba(OVERLAY))
                        .text_color(rgb(FG))
                        .hover(|s| s.bg(rgba(SELECT_MUTED)))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.choose_download_dir(cx)
                        }))
                        .child("Choose…"),
                ),
        )
    }

    fn refresh_setting(&self, cx: &mut Context<Self>) -> impl IntoElement {
        setting_block(
            "Refresh interval",
            "How often open folders revalidate in the background.",
            h_flex()
                .gap_1()
                .child(self.refresh_preset(5, cx))
                .child(self.refresh_preset(15, cx))
                .child(self.refresh_preset(30, cx))
                .child(self.refresh_preset(60, cx)),
        )
    }

    fn refresh_preset(&self, secs: u64, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.store.get().refresh_secs == secs;
        let base = h_flex()
            .id(SharedString::from(format!("preset-{secs}")))
            .px_3()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_color(if active { rgb(FG) } else { rgb(FG_MUTED) })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_refresh(secs, cx)))
            .child(format!("{secs}s"));
        if active {
            base.bg(rgba(SELECT))
        } else {
            base.hover(|s| s.bg(rgba(OVERLAY)))
        }
    }

    fn settings_info(&self) -> impl IntoElement {
        v_flex()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(rgb(BORDER_MUTED))
            .child(info_row("rclone", &self.version))
            .child(info_row("Data", &self.paths.root().display().to_string()))
            .child(info_row("Config", &self.paths.config_dir().display().to_string()))
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
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(rgb(BORDER_MUTED))
            .bg(rgb(INSET))
            .text_xs()
            .text_color(rgb(FG_MUTED))
            .child(
                h_flex().gap_2().child(self.health_dot()).children(self.active_remote().map(|r| {
                    h_flex()
                        .gap_2()
                        .child(div().text_color(rgb(FG)).child(r.name.clone()))
                        .child(div().text_color(rgb(FG_SUBTLE)).child(r.kind.clone()))
                })),
            )
            .child(
                h_flex()
                    .gap_3()
                    .when(!self.jobs.is_empty(), |el| el.child(self.jobs_indicator(cx)))
                    .child(info)
                    .child(self.version.clone()),
            )
    }

    fn health_dot(&self) -> impl IntoElement {
        let (color, tip) = match &self.rc_health {
            RcHealth::Unknown => (FG_SUBTLE, "Checking rclone daemon…".to_string()),
            RcHealth::Up if self.version.is_empty() => (SUCCESS, "rclone rc daemon connected".to_string()),
            RcHealth::Up => (SUCCESS, format!("rclone {} · rc daemon connected", self.version)),
            RcHealth::Down(e) => (DANGER, format!("rclone rc daemon unreachable: {e}")),
        };
        h_flex()
            .id("rc-health")
            .child(div().text_color(rgb(color)).child("●"))
            .tooltip(tooltip_text(tip))
    }

    fn jobs_indicator(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Separate counts so a mixed run reads e.g. "↻2  ✓3  ⚠1".
        let active = self.jobs.iter().filter(|j| !j.done).count();
        let failed = self.jobs.iter().filter(|j| j.done && j.error.is_some()).count();
        let succeeded = self.jobs.iter().filter(|j| j.done && j.error.is_none()).count();
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
