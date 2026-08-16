//! The bottom status bar: daemon status button + popover, open-remote info,
//! job counts. The daemon health/logic lives in the `DaemonStatus` entity; this
//! renders it with the shared popover/menu helpers (consistent with the menus).

use super::*;

impl Workspace {
    pub(crate) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let info = if self.open_remote(cx).is_some() {
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
                    .when(self.is_split(), |el| el.child(self.sync_status(cx)))
                    .child(self.tasks_toggle(cx)),
            )
    }

    /// Status-bar sync button (only while split): opens the compare/sync popover
    /// anchored above it. Mirrors the daemon status button.
    fn sync_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.sync_popover_open();
        div()
            .relative()
            .child(
                h_flex()
                    .id("sync-status")
                    .gap_1()
                    .p(px(3.0))
                    .items_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(rgb(FG_MUTED))
                    .when(open, |el| el.bg(rgba(OVERLAY)))
                    .hover(|s| s.bg(rgba(OVERLAY)))
                    .child(
                        svg()
                            .path("icons/git_compare.svg")
                            .size(rem(13.0))
                            .flex_shrink_0()
                            .text_color(rgb(FG_MUTED)),
                    )
                    .child("Sync")
                    .when(!open, |el| el.tooltip(tooltip_text("Compare and sync the two panes")))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.close_menus();
                        this.menu = Some(ActiveMenu::SyncPopover);
                        cx.notify();
                    })),
            )
            .when(open, |el| {
                el.child(
                    deferred(
                        div().absolute().bottom_full().right_0().pb_1().child(
                            self.popover_surface(
                                "sync-popover",
                                vec![self.sync_pane.clone().into_any_element()],
                                cx,
                            )
                            .w(rem(360.0))
                            .rounded_sm(),
                        ),
                    )
                    .priority(3),
                )
            })
    }

    /// Status-bar daemon button: an icon whose color tracks health (red on
    /// error), opening the rcd popover anchored to this button. The tooltip is
    /// suppressed while the popover is open, like Zed's status-bar buttons.
    fn rc_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let popover = self.rc_popover();
        let open = popover.is_some();
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
                        let spec = this.rc_menu_spec(cx);
                        let (menu, sub) = this.build_menu(spec, cx);
                        this.menu = Some(ActiveMenu::RcPopover(menu, sub));
                        cx.notify();
                    })),
            )
            .when(open, |el| {
                el.child(
                    deferred(
                        div()
                            .absolute()
                            .bottom_full()
                            .left_0()
                            .pb_1()
                            .w(rem(220.0))
                            .children(popover.clone()),
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

    /// The daemon status + actions, rendered through [`ContextMenu`].
    pub(crate) fn rc_menu_spec(&self, cx: &mut Context<Self>) -> MenuSpec {
        let health = self.daemon.read(cx).health().clone();
        let (tint, status) = match &health {
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
        MenuSpec::new()
            .header(MenuHeader {
                icon: health.icon(),
                tint,
                title: "rclone daemon".into(),
                subtitle: subtitle.into(),
                error: match &health {
                    RcHealth::Down(e) => Some(e.clone().into()),
                    _ => None,
                },
            })
            .item("rc-reconnect", "Reconnect", "icons/activity.svg", |this, _, cx| {
                this.daemon.update(cx, |d, cx| d.reconnect(cx));
            })
            // Restarting already in flight: skip a redundant restart.
            .when(!matches!(health, RcHealth::Restarting), |m| {
                m.item("rc-restart", "Restart daemon", "icons/refresh.svg", |this, _, cx| {
                    this.daemon.update(cx, |d, cx| d.restart(cx));
                })
            })
            .item("rc-copy-logs", "Copy logs path", "icons/copy.svg", move |this, _, cx| {
                this.copy_to_clipboard(logs.clone(), cx)
            })
    }

    /// A status-bar dock-panel toggle: an icon, highlighted when its panel shows,
    /// like Zed's bottom-right panel buttons. `extra` adds trailing content (the
    /// Tasks toggle's live job badges).
    fn panel_toggle(
        &self,
        id: &'static str,
        icon: &'static str,
        label: &'static str,
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
            .gap_1()
            .px_1p5()
            .py(px(2.0))
            .rounded_md()
            .cursor_pointer()
            .text_color(rgb(color))
            .when(on, |el| el.bg(rgba(SELECT_MUTED)))
            .hover(|s| s.bg(rgba(OVERLAY)))
            .tooltip(tooltip_text(tip))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| on_click(this, window, cx)))
            .child(svg().path(icon).size(rem(14.0)).flex_shrink_0().text_color(rgb(color)))
            .child(label)
            .children(extra)
    }

    fn tasks_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Only the actionable counts in the status bar — running and failed; the
        // full breakdown lives in the Tasks panel.
        let jobs = self.jobs.read(cx);
        let active = jobs.items().iter().filter(|j| !j.done).count();
        let failed = jobs.items().iter().filter(|j| j.done && j.error.is_some()).count();
        let badges = (active > 0 || failed > 0).then(|| {
            h_flex()
                .gap_1()
                .when(active > 0, |el| el.child(notification_badge(active, ACCENT, ACCENT_SOFT)))
                .when(failed > 0, |el| el.child(notification_badge(failed, DANGER, DANGER_SOFT)))
                .into_any_element()
        });
        let tip = format!("Tasks ({})", if cfg!(target_os = "macos") { "\u{2318}T" } else { "Ctrl T" });
        self.panel_toggle(
            "tasks-toggle",
            "icons/tasks.svg",
            "Tasks",
            self.dock_is(Panel::Tasks),
            true,
            tip,
            |this, _, cx| this.toggle_panel(Panel::Tasks, cx),
            badges,
            cx,
        )
    }

}
