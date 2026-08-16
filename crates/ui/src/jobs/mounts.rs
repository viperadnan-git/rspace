//! Mount / unmount lifecycle and per-remote mount config.

use super::*;

impl Workspace {
    /// Mount `remote` (no-install NFS) if unmounted, else unmount it; a pending
    /// toast tracks the result. The op result is authoritative, so the cached
    /// `mounted` set is updated from it directly (no extra service round-trip).
    pub(crate) fn toggle_mount(&mut self, remote: String, cx: &mut Context<Self>) {
        let mounting = !self.mounted.contains(&remote);
        let Some(mountpoint) = mount_root().map(|r| r.join(&remote)) else {
            self.toast_sticky("Cannot determine a mount location", true, cx);
            return;
        };
        let verb = if mounting { "Mounting" } else { "Unmounting" };
        let pending = self.toast_pending(format!("{verb} {remote}\u{2026}"), cx);
        let config = self.mount_config_for(&remote);
        let service = self.app.service.clone();
        let ok_msg = format!("{remote} {}", if mounting { "mounted" } else { "unmounted" });
        let op = {
            let remote = remote.clone();
            async move {
                if mounting {
                    service.mount_remote(remote, mountpoint, config).await
                } else {
                    service.unmount_remote(remote).await
                }
            }
        };
        self.track_mount(remote, mounting, ok_msg, pending, op, cx);
    }

    /// Spawn a mount/unmount `op`, then reconcile the cached `mounted` set and
    /// resolve `pending` from its result (`mounted_on_ok` = membership on success).
    fn track_mount(
        &mut self,
        remote: String,
        mounted_on_ok: bool,
        ok_msg: String,
        pending: usize,
        op: impl std::future::Future<Output = Result<(), ServiceError>> + 'static,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = op.await;
            this.update(cx, |this, cx| {
                let body = match &result {
                    Ok(()) => {
                        if mounted_on_ok {
                            this.mounted.insert(remote);
                        } else {
                            this.mounted.remove(&remote);
                        }
                        ToastBody::Message { message: ok_msg.into(), danger: false }
                    }
                    Err(e) => ToastBody::Message { message: format!("{e}").into(), danger: true },
                };
                this.resolve_toast(pending, body, true, cx);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn mount_config_for(&self, remote: &str) -> MountConfig {
        self.mount_configs.get(remote).cloned().unwrap_or_default()
    }

    /// Persist `config` for `remote`; remount live remotes so the flags apply.
    pub(crate) fn apply_mount_config(
        &mut self,
        remote: String,
        config: MountConfig,
        cx: &mut Context<Self>,
    ) {
        if let Ok(json) = serde_json::to_string(&config) {
            self.app.db.save_mount_config(&remote, &json);
        }
        self.mount_configs.insert(remote.clone(), config);
        if self.mounted.contains(&remote) {
            self.remount(remote, cx);
        }
        cx.notify();
    }

    /// Unmount then re-mount `remote` with its current config (to apply changed
    /// flags, which are fixed at mount time).
    fn remount(&mut self, remote: String, cx: &mut Context<Self>) {
        let Some(mountpoint) = mount_root().map(|r| r.join(&remote)) else {
            return;
        };
        let config = self.mount_config_for(&remote);
        let pending = self.toast_pending(format!("Remounting {remote}\u{2026}"), cx);
        let service = self.app.service.clone();
        let ok_msg = format!("{remote} remounted");
        let op = {
            let remote = remote.clone();
            async move {
                let _ = service.unmount_remote(remote.clone()).await;
                service.mount_remote(remote, mountpoint, config).await
            }
        };
        self.track_mount(remote, true, ok_msg, pending, op, cx);
    }

    pub(crate) fn reveal_mount(&self, remote: &str, cx: &mut Context<Self>) {
        if let Some(mountpoint) = mount_root().map(|r| r.join(remote)) {
            cx.open_with_system(&mountpoint);
        }
    }
}
