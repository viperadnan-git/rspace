//! In-app update check via `cargo-packager-updater`, against the signed bundles
//! attached to each GitHub Release. Verifies the update signature with the
//! embedded public key; if updates aren't wired yet (no pubkey) or the check
//! fails, it falls back to opening the releases page.

use cargo_packager_updater::{check_update, Config};

use super::*;

/// Releases page — the fallback when self-update isn't possible.
const RELEASES_URL: &str = "https://github.com/viperadnan-git/rspace/releases/latest";

/// Multi-platform update manifest published on each release; the updater picks
/// its own `{target}-{arch}` entry (e.g. `macos-aarch64`).
const UPDATE_ENDPOINT: &str =
    "https://github.com/viperadnan-git/rspace/releases/latest/download/latest.json";

/// Public half of the updater signing key (`cargo packager signer generate`).
/// The private key + password live in the repo secrets the packager workflow
/// signs with; if this is ever blanked, self-update falls back to releases.
const UPDATER_PUBKEY: &str =
    "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDhEQ0EzQjRCMUQ5ODAwRUIKUldUckFKZ2RTenZLamZvTXBZQkVNMkx4OXR2NjlsR1kzeGVrZmpRUlhvbEZHRnc0aXFCSWc3RmIK";

enum UpdateOutcome {
    UpToDate,
    Updated(String),
    /// Self-update not configured or the check failed — open releases.
    Unavailable,
}

impl Workspace {
    pub(crate) fn check_for_updates(
        &mut self,
        _: &CheckForUpdates,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toast("Checking for updates\u{2026}", false, cx);
        cx.spawn(async move |this, cx| {
            // The updater downloads + installs inline, so run it off the main thread.
            let outcome = cx.background_executor().spawn(async move { check_and_install() }).await;
            this.update(cx, |this, cx| match outcome {
                UpdateOutcome::UpToDate => this.toast("rspace is up to date", false, cx),
                UpdateOutcome::Updated(v) => {
                    this.toast_sticky(format!("Updated to {v} — restart to apply"), false, cx)
                }
                UpdateOutcome::Unavailable => cx.open_url(RELEASES_URL),
            })
            .ok();
        })
        .detach();
    }
}

/// Check for a newer signed release and install it if one exists.
fn check_and_install() -> UpdateOutcome {
    if UPDATER_PUBKEY.is_empty() {
        return UpdateOutcome::Unavailable;
    }
    let config = Config {
        endpoints: vec![UPDATE_ENDPOINT.parse().expect("valid update endpoint")],
        pubkey: UPDATER_PUBKEY.into(),
        ..Default::default()
    };
    let current = env!("CARGO_PKG_VERSION").parse().expect("valid crate version");
    match check_update(current, config) {
        Ok(Some(update)) => {
            let version = update.version.to_string();
            match update.download_and_install() {
                Ok(_) => UpdateOutcome::Updated(version),
                Err(_) => UpdateOutcome::Unavailable,
            }
        }
        Ok(None) => UpdateOutcome::UpToDate,
        Err(_) => UpdateOutcome::Unavailable,
    }
}
