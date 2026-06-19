//! The transfers / job-history side panel.

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
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.reveal_target(target.clone(), window, cx)))
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
}

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
