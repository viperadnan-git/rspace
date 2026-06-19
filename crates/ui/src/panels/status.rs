//! The bottom status bar: daemon status button + popover, open-remote info,
//! job counts. The daemon health/logic lives in the `DaemonStatus` entity; this
//! renders it with the shared popover/menu helpers (consistent with the menus).

use super::*;

impl Workspace {
    pub(crate) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let info = if self.open_remote.is_some() {
            let exp = self.explorer.read(cx);
            if exp.selection_len() > 1 {
                format!("{} selected", exp.selection_len())
            } else {
                format!("{} items", exp.entries().len())
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
        let open = self.menus.rc_popover_open;
        let health = self.daemon.read(cx).health().clone();
        let (color, tip): (u32, SharedString) = match &health {
            RcHealth::Up => (FG_MUTED, "rclone rc daemon connected".into()),
            RcHealth::Down(e) => (DANGER, format!("rclone rc daemon unreachable: {e}").into()),
            RcHealth::Restarting => (FG_MUTED, "Restarting rclone daemon…".into()),
            RcHealth::Unknown => (FG_SUBTLE, "Checking rclone daemon…".into()),
        };
        let icon: AnyElement = if matches!(health, RcHealth::Restarting) {
            spinner("rc-spin", px(15.0), FG_MUTED).into_any_element()
        } else {
            svg().path(health.icon()).size(px(15.0)).flex_shrink_0().text_color(rgb(color)).into_any_element()
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
                        this.menus.rc_popover_open = true;
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
    /// the workspace root, below the popover card.
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
        let health = self.daemon.read(cx).health().clone();
        let (color, status) = match &health {
            RcHealth::Unknown => (FG_SUBTLE, "Connecting…"),
            RcHealth::Up => (SUCCESS, "Connected"),
            RcHealth::Down(_) => (DANGER, "Disconnected"),
            RcHealth::Restarting => (FG_MUTED, "Restarting…"),
        };
        let subtitle = match (&health, self.version.is_empty()) {
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
                        .child(svg().path(health.icon()).size(px(14.0)).flex_shrink_0().text_color(rgb(color)))
                        .child(div().text_color(rgb(FG)).child("rclone daemon")),
                )
                .child(div().text_xs().text_color(rgb(FG_MUTED)).child(subtitle))
                .into_any_element(),
        );
        if let RcHealth::Down(e) = &health {
            items.push(
                div().w_full().px_2().pb_1().text_xs().text_color(rgb(DANGER)).child(e.clone()).into_any_element(),
            );
        }
        items.push(div().w_full().my_1().h(px(1.0)).bg(rgb(BORDER_MUTED)).into_any_element());
        items.push(
            self.menu_item("Reconnect", "icons/activity.svg", cx, |this, cx| {
                this.daemon.update(cx, |d, cx| d.reconnect(cx));
            })
            .into_any_element(),
        );
        // Restarting already in flight: skip a redundant restart.
        if !matches!(health, RcHealth::Restarting) {
            items.push(
                self.menu_item("Restart daemon", "icons/refresh.svg", cx, |this, cx| {
                    this.daemon.update(cx, |d, cx| d.restart(cx));
                })
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
