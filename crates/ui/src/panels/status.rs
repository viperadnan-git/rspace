//! The bottom status bar: daemon status button + popover, open-remote info,
//! job counts. The daemon health/logic lives in the `DaemonStatus` entity; this
//! renders it with the shared popover/menu helpers (consistent with the menus).

use super::*;

impl Workspace {
    pub(crate) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let info = if self.active().open_remote.is_some() {
            let exp = self.explorer();
            let exp = exp.read(cx);
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
            // Fixed height so job badges appearing/disappearing can't shift the
            // layout; rem-based so it scales with the UI font.
            .h(rem(28.0))
            .overflow_hidden()
            .justify_between()
            .items_center()
            .pl_1()
            .pr_1()
            .border_t_1()
            .border_color(rgb(BORDER_MUTED))
            .bg(rgb(INSET))
            .text_xs()
            .text_color(rgb(FG_MUTED))
            .child(
                // `version` already reads "rclone vX.Y", so no prefix.
                h_flex().gap_2().items_center().child(self.rc_status(cx)).when(
                    !self.version.is_empty(),
                    |el| el.child(div().text_color(rgb(FG_SUBTLE)).child(self.version.clone())),
                ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(div().px_1().child(info))
                    .child(self.tasks_toggle(cx)),
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
            svg().path(health.icon()).size(rem(15.0)).flex_shrink_0().text_color(rgb(color)).into_any_element()
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
        let logs = self.app.paths.logs_dir().to_string_lossy().into_owned();
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
                        .child(svg().path(health.icon()).size(rem(14.0)).flex_shrink_0().text_color(rgb(color)))
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
            self.menu_item("Reconnect", "icons/activity.svg", cx, |this, _, cx| {
                this.daemon.update(cx, |d, cx| d.reconnect(cx));
            })
            .into_any_element(),
        );
        // Restarting already in flight: skip a redundant restart.
        if !matches!(health, RcHealth::Restarting) {
            items.push(
                self.menu_item("Restart daemon", "icons/refresh.svg", cx, |this, _, cx| {
                    this.daemon.update(cx, |d, cx| d.restart(cx));
                })
                .into_any_element(),
            );
        }
        items.push(
            self.menu_item("Copy logs path", "icons/copy.svg", cx, move |this, _, cx| {
                this.copy_to_clipboard(logs.clone(), cx)
            })
            .into_any_element(),
        );
        self.popover_surface("rc-popover", items, cx).w(rem(220.0))
    }

    /// A status-bar dock-panel toggle: an icon, highlighted when its panel shows,
    /// like Zed's bottom-right panel buttons. `extra` adds trailing content (the
    /// Tasks toggle's live job badges).
    fn panel_toggle(
        &self,
        id: &'static str,
        icon: &'static str,
        on: bool,
        enabled: bool,
        tip: impl Into<SharedString>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        extra: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let color = if on { FG } else if enabled { FG_MUTED } else { FG_SUBTLE };
        let tip: SharedString = tip.into();
        h_flex()
            .id(id)
            .gap_1p5()
            .px_1p5()
            .py(px(2.0))
            .rounded_md()
            .cursor_pointer()
            .when(on, |el| el.bg(rgba(SELECT_MUTED)))
            .hover(|s| s.bg(rgba(OVERLAY)))
            .tooltip(tooltip_text(tip))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| on_click(this, window, cx)))
            .child(svg().path(icon).size(rem(14.0)).flex_shrink_0().text_color(rgb(color)))
            .children(extra)
    }

    fn tasks_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Separate counts so a mixed run reads e.g. "↻2  ✓3  ⚠1".
        let jobs = self.jobs.read(cx);
        let active = jobs.items().iter().filter(|j| !j.done).count();
        let failed = jobs.items().iter().filter(|j| j.done && j.error.is_some()).count();
        let succeeded = jobs.items().iter().filter(|j| j.done && j.error.is_none()).count();
        let badges = (active > 0 || succeeded > 0 || failed > 0).then(|| {
            h_flex()
                .gap_1p5()
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
                .into_any_element()
        });
        let tip = format!("Tasks ({})", if cfg!(target_os = "macos") { "\u{2318}T" } else { "Ctrl T" });
        self.panel_toggle(
            "tasks-toggle",
            "icons/tasks.svg",
            self.dock_is(Panel::Tasks),
            true,
            tip,
            |this, _, cx| this.toggle_panel(Panel::Tasks, cx),
            badges,
            cx,
        )
    }

}
