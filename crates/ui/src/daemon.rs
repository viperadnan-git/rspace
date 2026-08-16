//! The rcd (rclone rc daemon) connection: health polling and recovery. A model
//! entity owned by the workspace — it polls the daemon and exposes its health,
//! reaching back only to re-sync the views on recovery. The status-bar button
//! and popover that surface it are rendered by `panels::status`.

use gpui::WeakEntity;

use super::*;

/// Reachability of the rclone rc daemon.
#[derive(Clone)]
pub(crate) enum RcHealth {
    Unknown,
    Up,
    Down(String),
    /// Daemon is being restarted (a fresh `rcd` is spawning).
    Restarting,
}

impl RcHealth {
    /// The rclone brand mark for the daemon button — colored by the caller
    /// (normal when up, red on error).
    pub(crate) fn icon(&self) -> &'static str {
        "icons/rclone.svg"
    }
}

pub(crate) struct DaemonStatus {
    workspace: WeakEntity<Workspace>,
    service: Service,
    health: RcHealth,
}

impl DaemonStatus {
    pub(crate) fn new(
        workspace: WeakEntity<Workspace>,
        service: Service,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Ping on an interval for the status dot; runs unfocused.
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let service = match cx.update(|_, app| this.update(app, |v, _| v.service.clone()).ok()) {
                    Ok(Some(s)) => s,
                    _ => break,
                };
                let health = match service.ping().await {
                    Ok(()) => RcHealth::Up,
                    Err(e) => RcHealth::Down(e.to_string()),
                };
                let alive = cx
                    .update(|_, app| {
                        this.update(app, |this, cx| {
                            // Don't fight an in-flight restart (it pings the old, dead port).
                            if !matches!(this.health, RcHealth::Restarting) {
                                this.health = health;
                                cx.notify();
                            }
                        })
                        .is_ok()
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(3)).await;
            }
        })
        .detach();
        Self { workspace, service, health: RcHealth::Unknown }
    }

    pub(crate) fn health(&self) -> &RcHealth {
        &self.health
    }

    /// Mark the daemon healthy and re-sync the workspace views.
    fn on_daemon_up(&mut self, cx: &mut Context<Self>) {
        self.health = RcHealth::Up;
        self.workspace
            .update(cx, |ws, cx| {
                ws.load_remotes(cx);
                if ws.open_remote(cx).is_some() {
                    ws.force_reload_entries(cx);
                }
            })
            .ok();
    }

    /// Re-ping the daemon and refresh the listings (recover a lost connection).
    pub(crate) fn reconnect(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        cx.spawn(async move |this, cx| {
            let result = service.ping().await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.on_daemon_up(cx),
                    Err(e) => this.health = RcHealth::Down(e.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Stop and respawn `rcd`, then refresh. The swap-able client means every
    /// in-flight and future call picks up the new endpoint automatically.
    pub(crate) fn restart(&mut self, cx: &mut Context<Self>) {
        self.health = RcHealth::Restarting;
        cx.notify();
        let service = self.service.clone();
        cx.spawn(async move |this, cx| {
            let result = service.restart_daemon().await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.on_daemon_up(cx),
                    Err(e) => this.health = RcHealth::Down(e.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
