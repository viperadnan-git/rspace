//! The Tasks panel as a first-class entity (like [`Explorer`]/[`Sidebar`]): it
//! owns the job-list rendering, the multi-selection, and keyboard focus. Side
//! effects — retry/cancel/remove, reveal, confirm dialogs, dock — stay on the
//! [`Workspace`]; this reaches them through the weak handle. It reads the shared
//! [`Jobs`] entity and re-renders when it changes.

use gpui::{Entity, WeakEntity};

use super::*;

pub(crate) struct TasksPane {
    workspace: WeakEntity<Workspace>,
    jobs: Entity<Jobs>,
    focus: FocusHandle,
    /// Multi-selection by job id; pruned against live jobs each render.
    sel: Selection<usize>,
    _obs: gpui::Subscription,
}

impl Focusable for TasksPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TasksPane {
    pub(crate) fn new(
        workspace: WeakEntity<Workspace>,
        jobs: Entity<Jobs>,
        cx: &mut Context<Self>,
    ) -> Self {
        let obs = cx.observe(&jobs, |_, _, cx| cx.notify());
        Self { workspace, jobs, focus: cx.focus_handle(), sel: Selection::new(), _obs: obs }
    }

    // --- selection ------------------------------------------------------------

    /// Job ids in render order (newest first), for shift-range and select-all.
    fn ordered_ids(&self, cx: &App) -> Vec<usize> {
        self.jobs.read(cx).items().iter().rev().map(|j| j.id).collect()
    }

    fn select_only(&mut self, id: usize, cx: &mut Context<Self>) {
        self.sel.select_only(id);
        cx.notify();
    }

    fn toggle(&mut self, id: usize, cx: &mut Context<Self>) {
        self.sel.toggle(id);
        cx.notify();
    }

    fn range_to(&mut self, id: usize, cx: &mut Context<Self>) {
        let order = self.ordered_ids(cx);
        self.sel.range_to(&order, id);
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        if !self.sel.is_empty() {
            self.sel.clear();
            cx.notify();
        }
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let order = self.ordered_ids(cx);
        self.sel.all(&order);
        cx.notify();
    }

    // --- rendering ------------------------------------------------------------

    fn target_chip(
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
            .on_mouse_down(MouseButton::Left, cx.listener(|_, _: &MouseDownEvent, _, cx| cx.stop_propagation()))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                let target = target.clone();
                this.workspace.update(cx, |ws, cx| ws.reveal_target(target, window, cx)).ok();
            }))
            .child(label.into())
    }

    fn job_row(&self, job: &Job, selected: bool, cx: &mut Context<Self>) -> AnyElement {
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
        let (can_retry, can_remove) = (job.done && job.error.is_some(), job.done);
        let action_button = move |suffix: &str, svg_icon: &'static str, tip: &'static str| {
            icon_button(SharedString::from(format!("{suffix}-{id}")), svg_icon).tooltip(tooltip_text(tip))
        };
        // Cancel lives in the row's right-click menu, not as an inline button.
        // Inner controls swallow the left press so they don't also change selection.
        let actions = h_flex()
            .flex_shrink_0()
            .gap_0p5()
            .items_center()
            .on_mouse_down(MouseButton::Left, cx.listener(|_, _: &MouseDownEvent, _, cx| cx.stop_propagation()))
            .when(can_retry, |el| {
                el.child(action_button("retry", "icons/refresh.svg", "Retry").on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.workspace.update(cx, |ws, cx| ws.retry_job(id, cx)).ok();
                    }),
                ))
            })
            .when(can_remove, |el| {
                el.child(action_button("clear", "icons/trash.svg", "Remove").on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.workspace.update(cx, |ws, cx| ws.clear_job(id, cx)).ok();
                    }),
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
            id,
            selected,
        };
        self.task_row(data, actions, cx)
    }

    fn task_row(&self, d: RowData, actions: AnyElement, cx: &mut Context<Self>) -> AnyElement {
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
            id,
            selected,
        } = d;
        let pct = if total > 0 { (bytes as f64 / total as f64).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let time = human_duration(elapsed_ms);
        let head = targets.last();
        let name = head.map(|t| t.name.clone()).unwrap_or_default();

        let icon: AnyElement = match &status {
            TaskStatus::Running => spinner(SharedString::from(format!("{key}-spin")), px(15.0), ACCENT).into_any_element(),
            TaskStatus::Done => svg().path("icons/check.svg").size(rem(15.0)).text_color(rgb(SUCCESS)).into_any_element(),
            TaskStatus::Cancelled => svg().path("icons/x.svg").size(rem(15.0)).text_color(rgb(FG_MUTED)).into_any_element(),
            TaskStatus::Failed(_) => svg().path("icons/alert.svg").size(rem(15.0)).text_color(rgb(DANGER)).into_any_element(),
        };

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

        let wash = match &status {
            TaskStatus::Failed(_) => Some((DANGER_SOFT, 1.0_f32)),
            TaskStatus::Running if total > 0 => Some((ACCENT_SOFT, pct)),
            TaskStatus::Cancelled if total > 0 => Some((OVERLAY, pct)),
            _ => None,
        };

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
            .when(selected, |el| el.bg(rgba(SELECT_MUTED)))
            // stop propagation so the outer empty-space handler doesn't clear selection
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.focus.focus(window, cx);
                    let m = e.modifiers;
                    if m.secondary() {
                        this.toggle(id, cx);
                    } else if m.shift {
                        this.range_to(id, cx);
                    } else {
                        this.select_only(id, cx);
                    }
                }),
            )
            // Finder-style: right-click selects the row first if it wasn't already selected.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if !this.sel.contains(&id) {
                        this.sel.select_only(id);
                    }
                    let ids: Vec<usize> = this.sel.iter().copied().collect();
                    let pos = e.position;
                    this.workspace.update(cx, |ws, cx| ws.open_task_menu(ids, pos, cx)).ok();
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
                            self.target_chip(SharedString::from(format!("{key}-name")), t.clone(), name, cx)
                        })),
                    )
                    .child(div().flex_shrink_0().text_xs().text_color(rgb(primary_color)).child(primary))
                    .child(actions),
            )
            .child(secondary_line)
            .into_any_element()
    }
}

impl Render for TasksPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Self-heal the selection: drop ids whose jobs are gone (cleared/retried).
        let live: HashSet<usize> = self.jobs.read(cx).items().iter().map(|j| j.id).collect();
        self.sel.retain(|id| live.contains(id));

        let n = self.jobs.read(cx).items().len();
        if n == 0 {
            return centered("No tasks", FG_SUBTLE).into_any_element();
        }
        // Clone per row to avoid holding the borrow across the loop body.
        let rows: Vec<AnyElement> = (0..n)
            .rev()
            .map(|i| {
                let job = self.jobs.read(cx).items()[i].clone();
                let selected = self.sel.contains(&job.id);
                div()
                    .border_b_1()
                    .border_color(rgb(BORDER_MUTED))
                    .child(self.job_row(&job, selected, cx))
                    .into_any_element()
            })
            .collect();
        v_flex()
            .id("tasks")
            .track_focus(&self.focus)
            .key_context("Tasks")
            .on_action(cx.listener(Self::select_all))
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            // focus so Select-all routes here; row handlers stop propagation so clicks there skip this
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    this.clear(cx);
                }),
            )
            .children(rows)
            .into_any_element()
    }
}

/// Status of a task row.
enum TaskStatus {
    Running,
    Done,
    Cancelled,
    Failed(SharedString),
}

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
    id: usize,
    selected: bool,
}
