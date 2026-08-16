//! In-app updates via `cargo-packager-updater`. A newer version prompts a dialog
//! (Install / Later / Skip); artifacts are signature-verified with the embedded
//! public key. Manual checks fall back to the releases page.

use cargo_packager_updater::{check_update, Config};

use crate::components::update_modal::{UpdateChoice, UpdateModal};

use super::*;

/// Releases page — the fallback when self-update isn't possible.
const RELEASES_URL: &str = "https://github.com/viperadnan-git/rspace/releases/latest";

/// Multi-platform update manifest published on each release; the updater picks
/// its own `{target}-{arch}` entry (e.g. `macos-aarch64`).
const UPDATE_ENDPOINT: &str =
    "https://github.com/viperadnan-git/rspace/releases/latest/download/latest.json";

/// Public half of the updater signing key (`cargo packager signer generate`).
const UPDATER_PUBKEY: &str =
    "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDhEQ0EzQjRCMUQ5ODAwRUIKUldUckFKZ2RTenZLamZvTXBZQkVNMkx4OXR2NjlsR1kzeGVrZmpRUlhvbEZHRnc0aXFCSWc3RmIK";

/// A newer release the updater found.
struct Available {
    version: String,
    notes: Option<String>,
}

fn updater_config() -> Option<Config> {
    if UPDATER_PUBKEY.is_empty() {
        return None;
    }
    Some(Config {
        endpoints: vec![UPDATE_ENDPOINT.parse().ok()?],
        pubkey: UPDATER_PUBKEY.into(),
        ..Default::default()
    })
}

/// Blocking; run off the main thread.
fn check() -> Option<Available> {
    let current = env!("CARGO_PKG_VERSION").parse().ok()?;
    let update = check_update(current, updater_config()?).ok()??;
    Some(Available { version: update.version.to_string(), notes: update.body.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hits the live latest.json: fetch + parse + pubkey + version compare.
    #[test]
    #[ignore = "network"]
    fn updater_sees_the_published_release() {
        let config = updater_config().expect("pubkey embedded");
        let update = check_update("0.0.1".parse().unwrap(), config).expect("check succeeds");
        assert!(update.is_some(), "an update is available vs 0.0.1");
    }
}

/// Blocking; run off the main thread.
fn install() -> bool {
    let Some(config) = updater_config() else { return false };
    let Ok(current) = env!("CARGO_PKG_VERSION").parse() else { return false };
    matches!(check_update(current, config), Ok(Some(update)) if update.download_and_install().is_ok())
}

impl Workspace {
    pub(crate) fn check_for_updates(&mut self, _: &CheckForUpdates, _: &mut Window, cx: &mut Context<Self>) {
        self.toast("Checking for updates\u{2026}", false, cx);
        cx.spawn(async move |this, cx| {
            let found = cx.background_executor().spawn(async { check() }).await;
            this.update(cx, |this, cx| match found {
                Some(a) => this.prompt_update(a, cx),
                None => this.toast("rspace is up to date", false, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Launch check: prompt only for a newer, non-skipped version; silent otherwise.
    pub(crate) fn check_updates_on_startup(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let found = cx.background_executor().spawn(async { check() }).await;
            this.update(cx, |this, cx| {
                if let Some(a) = found {
                    let skipped = this.store.get().skipped_update_version.clone();
                    if skipped.as_deref() != Some(a.version.as_str()) {
                        this.prompt_update(a, cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn prompt_update(&mut self, avail: Available, cx: &mut Context<Self>) {
        let version = avail.version.clone();
        let notes = avail.notes.map(SharedString::from);
        let modal = cx.new(|cx| UpdateModal::new(version.clone(), notes, cx));
        let sub = cx.subscribe(&modal, move |this, _, choice, cx| {
            this.modal = None;
            match choice {
                UpdateChoice::Install => this.install_update(cx),
                UpdateChoice::Later => {}
                UpdateChoice::Skip => this.store.update(|s| s.skipped_update_version = Some(version.clone())),
            }
            cx.notify();
        });
        self.show_modal(ActiveModal::new(modal).deferred().subscribe(sub), cx);
    }

    fn install_update(&mut self, cx: &mut Context<Self>) {
        self.toast("Downloading update\u{2026}", false, cx);
        cx.spawn(async move |this, cx| {
            let ok = cx.background_executor().spawn(async { install() }).await;
            this.update(cx, |this, cx| {
                if ok {
                    this.toast_sticky("Update installed \u{2014} restart rspace to apply", false, cx);
                } else {
                    this.toast("Update failed \u{2014} download from the releases page", true, cx);
                    cx.open_url(RELEASES_URL);
                }
            })
            .ok();
        })
        .detach();
    }
}
