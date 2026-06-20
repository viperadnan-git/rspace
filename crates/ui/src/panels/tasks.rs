//! The Tasks panel: live (in-memory) job progress.

use super::*;

impl Workspace {
    /// The Tasks panel body (the dock supplies width, header, and close). A plain
    /// scrollable list (not `uniform_list`) so rows can be variable height — the
    /// second line wraps instead of clipping.
    pub(crate) fn render_tasks_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let n = self.jobs.read(cx).items().len();
        if n == 0 {
            return centered("No tasks", FG_SUBTLE).into_any_element();
        }
        // Newest first. Clone one job per row (releasing the borrow each time)
        // rather than the whole list; history is capped so `n` stays bounded.
        let rows: Vec<AnyElement> = (0..n)
            .rev()
            .map(|i| {
                let job = self.jobs.read(cx).items()[i].clone();
                div()
                    .border_b_1()
                    .border_color(rgb(BORDER_MUTED))
                    .child(self.job_row(&job, cx))
                    .into_any_element()
            })
            .collect();
        v_flex()
            .id("tasks")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .children(rows)
            .into_any_element()
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
        // Cancel lives in the row's right-click menu, not as an inline button.
        let actions = h_flex()
            .flex_shrink_0()
            .gap_0p5()
            .items_center()
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

        // Line-1 primary metric (+ color) and muted line-2 metadata, per state, in
        // one pass. A failed job's line 2 is its error instead (handled below).
        let (primary, primary_color, secondary): (SharedString, u32, String) = match &status {
            TaskStatus::Running if total > 0 => {
                let speed_s = (speed > 0.0)
                    .then(|| format!(" · {}/s", human_size(speed as i64)))
                    .unwrap_or_default();
                let eta = (speed > 0.0 && total > bytes)
                    .then(|| format!(" · {} left", human_duration(((total - bytes) as f64 / speed * 1000.0) as u64)))
                    .unwrap_or_default();
                let files = (total_transfers > 1)
                    .then(|| format!(" · {transfers}/{total_transfers} files"))
                    .unwrap_or_default();
                (
                    format!("{}%", (pct * 100.0).round() as u32).into(),
                    FG,
                    format!("{verb} · {} of {}{speed_s}{eta}{files}", human_size(bytes as i64), human_size(total as i64)),
                )
            }
            TaskStatus::Running => (human_size(bytes as i64).into(), FG, format!("{verb} · starting…")),
            TaskStatus::Done if total_transfers > 1 => (
                format!("{total_transfers} files").into(),
                FG_SUBTLE,
                format!("{verb} · {} · {time}", human_size(total as i64)),
            ),
            TaskStatus::Done => (human_size(total as i64).into(), FG_SUBTLE, format!("{verb} · {time}")),
            TaskStatus::Cancelled if total > 0 => (
                "Cancelled".into(),
                FG_MUTED,
                format!("{verb} · {} of {} · {time}", human_size(bytes as i64), human_size(total as i64)),
            ),
            TaskStatus::Cancelled => ("Cancelled".into(), FG_MUTED, format!("{verb} · cancelled · {time}")),
            TaskStatus::Failed(_) => ("Failed".into(), DANGER, String::new()),
        };

        // Ambient wash: accent→pct while running, full danger on failure, faint
        // neutral→pct for a cancel (shows how far it got).
        let wash = match &status {
            TaskStatus::Failed(_) => Some((DANGER_SOFT, 1.0_f32)),
            TaskStatus::Running if total > 0 => Some((ACCENT_SOFT, pct)),
            TaskStatus::Cancelled if total > 0 => Some((OVERLAY, pct)),
            _ => None,
        };

        // The error wraps to full width; every other state is one ellipsized line.
        let secondary_line: AnyElement = match &status {
            TaskStatus::Failed(msg) => {
                div().w_full().text_xs().text_color(rgb(DANGER)).child(msg.clone()).into_any_element()
            }
            _ => div()
                .w_full()
                .min_w(px(0.0))
                .truncate()
                .text_xs()
                .text_color(rgb(FG_SUBTLE))
                .child(secondary)
                .into_any_element(),
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
            .child(
                h_flex()
                    .relative()
                    .w_full()
                    .gap_1p5()
                    .items_center()
                    .child(div().flex_shrink_0().child(icon))
                    .child(
                        h_flex().flex_1().min_w(px(0.0)).text_color(rgb(FG)).children(head.map(|t| {
                            self.job_target_chip(SharedString::from(format!("{key}-name")), t.clone(), name, cx)
                        })),
                    )
                    .child(div().flex_shrink_0().text_xs().text_color(rgb(primary_color)).child(primary))
                    .child(actions),
            )
            .child(secondary_line)
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
