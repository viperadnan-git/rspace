//! The Tasks panel: live (in-memory) job progress.

use super::*;

impl Workspace {
    pub(crate) fn render_tasks(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_done = self.jobs.read(cx).has_finished();
        let count = self.jobs.read(cx).items().len();
        let body = if count == 0 {
            centered("No tasks", FG_SUBTLE).into_any_element()
        } else {
            uniform_list(
                "tasks",
                count,
                cx.processor(|this, range: Range<usize>, _window, cx| {
                    // Newest first; clone only the visible window, not the whole list.
                    let visible: Vec<Job> = {
                        let jobs = this.jobs.read(cx);
                        let n = jobs.items().len();
                        range
                            .filter_map(|i| n.checked_sub(1 + i).and_then(|idx| jobs.items().get(idx).cloned()))
                            .collect()
                    };
                    visible
                        .into_iter()
                        .map(|job| {
                            div()
                                .border_b_1()
                                .border_color(rgb(BORDER_MUTED))
                                .child(this.job_row(&job, cx))
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .flex_1()
            .into_any_element()
        };

        v_flex()
            .relative()
            .w(self.jobs_width)
            .min_h(px(0.0))
            .flex_shrink_0()
            .overflow_hidden()
            .bg(rgb(INSET))
            .border_l_1()
            .border_color(rgb(BORDER_MUTED))
            .child(self.resize_handle("jobs-resize", ResizeTarget::Jobs, cx))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .px_3()
                    .py_1p5()
                    .border_b_1()
                    .border_color(rgb(BORDER_MUTED))
                    .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child("TASKS"))
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
                            .child(icon_button("tasks-close", "icons/x.svg").on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.set_dock(None, cx);
                                }),
                            )),
                    ),
            )
            .child(body)
    }

    /// A clickable job endpoint: reveals it in the explorer on click, full
    /// `remote:path` on hover. `label` is the text shown (the file name).
    fn job_target_chip(
        &self,
        el_id: SharedString,
        target: JobTarget,
        label: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let full_path = format!("{}:{}", target.remote, target.path);
        div()
            .id(el_id)
            .min_w(px(0.0))
            .px_1()
            .rounded_md()
            .truncate()
            .cursor_pointer()
            .hover(|s| s.bg(rgba(OVERLAY)))
            .tooltip(tooltip_text(full_path))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.reveal_target(target.clone(), window, cx)))
            .child(label.into())
    }

    /// A live job → row data + its live action buttons (cancel / retry / clear).
    fn job_row(&self, job: &Job, cx: &mut Context<Self>) -> AnyElement {
        let id = job.id;
        let status = if job.cancelled {
            TaskStatus::Cancelled
        } else if let Some(err) = &job.error {
            TaskStatus::Failed(err.clone().into())
        } else if job.done {
            TaskStatus::Done
        } else {
            TaskStatus::Running
        };
        let (running, can_retry, can_remove) =
            (!job.done, job.done && job.error.is_some(), job.done);
        let action_button = move |suffix: &str, svg_icon: &'static str, tip: &'static str| {
            icon_button(SharedString::from(format!("{suffix}-{id}")), svg_icon).tooltip(tooltip_text(tip))
        };
        let actions = h_flex()
            .gap_0p5()
            .items_center()
            .child(self.copy_button(
                SharedString::from(format!("copy-cmd-{id}")),
                CopySource::JobCommand(id),
                job.command.clone(),
                "Copy rclone command",
                cx,
            ))
            .when(running, |el| {
                el.child(action_button("cancel", "icons/x.svg", "Cancel").on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.request_cancel_job(id, cx)),
                ))
            })
            .when(can_retry, |el| {
                el.child(action_button("retry", "icons/refresh.svg", "Retry").on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.retry_job(id, cx)),
                ))
            })
            .when(can_remove, |el| {
                el.child(action_button("clear", "icons/trash.svg", "Remove").on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.clear_job(id, cx)),
                ))
            })
            .into_any_element();
        let data = RowData {
            key: SharedString::from(format!("job-{id}")),
            verb: job.verb.clone(),
            targets: job.targets.clone(),
            status,
            bytes: job.bytes,
            total: job.total,
            speed: job.speed,
            transfers: job.transfers,
            total_transfers: job.total_transfers,
            elapsed_ms: job.elapsed_ms,
            menu: TaskMenuData {
                job_id: id,
                command: job.command.clone(),
                targets: job.targets.clone(),
                running,
                can_retry,
                can_remove,
            },
        };
        self.render_task_row(data, actions, cx)
    }

    /// One task as a compact two-line cell. Progress is ambient: a wash sweeps the
    /// row background to the percent done (danger for a failure, neutral for a
    /// cancel). Line 1: status icon + name + metric; line 2: type badge + actions,
    /// with size/ETA pinned right.
    fn render_task_row(&self, d: RowData, actions: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let RowData {
            key,
            verb,
            targets,
            status,
            bytes,
            total,
            speed,
            transfers,
            total_transfers,
            elapsed_ms,
            menu,
        } = d;
        let running = matches!(status, TaskStatus::Running);
        let pct = if total > 0 { (bytes as f64 / total as f64).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let time = human_duration(elapsed_ms);
        // Title shows where it's going: the destination if there is one, else the
        // sole endpoint.
        let head = targets.last();
        let name = head.map(|t| t.name.clone()).unwrap_or_default();

        let icon: AnyElement = match &status {
            TaskStatus::Running => spinner(SharedString::from(format!("{key}-spin")), px(15.0), ACCENT).into_any_element(),
            TaskStatus::Done => svg().path("icons/check.svg").size(rem(15.0)).text_color(rgb(SUCCESS)).into_any_element(),
            TaskStatus::Cancelled => svg().path("icons/x.svg").size(rem(15.0)).text_color(rgb(FG_MUTED)).into_any_element(),
            TaskStatus::Failed(_) => svg().path("icons/alert.svg").size(rem(15.0)).text_color(rgb(DANGER)).into_any_element(),
        };

        // Line-1 trailing metric, by state.
        let trailing: AnyElement = match &status {
            TaskStatus::Failed(msg) => {
                div().min_w(px(0.0)).truncate().text_xs().text_color(rgb(DANGER)).child(msg.clone()).into_any_element()
            }
            TaskStatus::Cancelled => {
                div().flex_shrink_0().text_xs().text_color(rgb(FG_MUTED)).child("Cancelled").into_any_element()
            }
            TaskStatus::Done => {
                let label = if total_transfers > 1 {
                    format!("{total_transfers} files")
                } else {
                    human_size(total as i64)
                };
                div().flex_shrink_0().text_xs().text_color(rgb(FG_SUBTLE)).child(label).into_any_element()
            }
            TaskStatus::Running => h_flex()
                .flex_shrink_0()
                .gap_1p5()
                .items_baseline()
                .when(total > 0, |el| {
                    el.child(div().text_color(rgb(FG)).child(format!("{}%", (pct * 100.0).round() as u32)))
                })
                .when(speed > 0.0, |el| {
                    el.child(div().text_xs().text_color(rgb(FG_SUBTLE)).child(format!("{}/s", human_size(speed as i64))))
                })
                .into_any_element(),
        };

        // Line-2 trailing metric, by state.
        let meta = match &status {
            TaskStatus::Failed(_) => format!("failed · {time}"),
            TaskStatus::Cancelled => match total {
                0 => format!("cancelled · {time}"),
                t => format!("{} / {} · {time}", human_size(bytes as i64), human_size(t as i64)),
            },
            // The size lives on line 1 for a single-file done, so line 2 carries it
            // only for multi-file (where line 1 shows the count instead).
            TaskStatus::Done if total_transfers > 1 => format!("{} · {time}", human_size(total as i64)),
            TaskStatus::Done => time.clone(),
            TaskStatus::Running if total > 0 => {
                let eta = if speed > 0.0 && total > bytes {
                    let eta_ms = ((total - bytes) as f64 / speed * 1000.0) as u64;
                    format!(" · {} left", human_duration(eta_ms))
                } else {
                    String::new()
                };
                format!("{} / {}{eta}", human_size(bytes as i64), human_size(total as i64))
            }
            TaskStatus::Running => format!("starting… · {time}"),
        };
        // Running shows live file progress on line 2 (line 1 has %); the count for a
        // done task is already on line 1.
        let meta = match (total_transfers > 1).then_some(total_transfers) {
            Some(n) if running => format!("{transfers}/{n} files · {meta}"),
            _ => meta,
        };

        // Ambient wash: accent→pct while running, full danger on failure, faint
        // neutral→pct for a cancel (shows how far it got).
        let wash = match &status {
            TaskStatus::Failed(_) => Some((DANGER_SOFT, 1.0_f32)),
            TaskStatus::Running if total > 0 => Some((ACCENT_SOFT, pct)),
            TaskStatus::Cancelled if total > 0 => Some((OVERLAY, pct)),
            _ => None,
        };

        v_flex()
            .relative()
            .w_full()
            .px_3()
            .py_2()
            .gap_0p5()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    this.close_menus();
                    this.menus.task_menu = Some((menu.clone(), e.position));
                    cx.notify();
                }),
            )
            .when_some(wash, |el, (color, frac)| {
                el.child(div().absolute().top_0().bottom_0().left_0().w(relative(frac)).bg(rgba(color)))
            })
            // Line 1: status · name · trailing metric.
            .child(
                h_flex()
                    .relative()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(div().flex_shrink_0().child(icon))
                    .child(
                        h_flex()
                            .flex_grow(1.0)
                            .min_w(px(0.0))
                            .text_color(rgb(FG))
                            .children(head.map(|t| {
                                self.job_target_chip(SharedString::from(format!("{key}-name")), t.clone(), name, cx)
                            })),
                    )
                    .child(trailing),
            )
            // Line 2: type badge + actions left, size/ETA pinned right.
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .text_color(rgb(FG_SUBTLE))
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(rgba(SELECT_MUTED))
                                    .text_color(rgb(FG_MUTED))
                                    .child(verb),
                            )
                            .child(actions),
                    )
                    .child(div().flex_shrink_0().min_w(px(0.0)).truncate().text_color(rgb(FG_MUTED)).child(meta)),
            )
            .into_any_element()
    }
}

/// Status of a task row, shared by live jobs and persisted history.
enum TaskStatus {
    Running,
    Done,
    Cancelled,
    Failed(SharedString),
}

/// Everything [`Workspace::render_task_row`] needs from a live `Job`.
struct RowData {
    key: SharedString,
    /// Operation name shown as a badge on line 2 (Copy, Move, Delete…).
    verb: SharedString,
    targets: Vec<JobTarget>,
    status: TaskStatus,
    bytes: u64,
    total: u64,
    speed: f64,
    transfers: u64,
    total_transfers: u64,
    elapsed_ms: u64,
    /// What the row's right-click menu acts on.
    menu: TaskMenuData,
}
