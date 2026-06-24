//! In-app update check via `axoupdater`, against the GitHub Release `dist` builds.
//! Needs a build installed through dist's installer (which writes the receipt
//! axoupdater reads) and, on macOS, a signed + notarized release — otherwise the
//! check falls back to opening the releases page.

use axoupdater::AxoUpdater;

use super::*;

/// Releases page — the fallback when self-update isn't possible (no receipt).
const RELEASES_URL: &str = "https://github.com/viperadnan-git/rspace/releases/latest";

enum UpdateOutcome {
    UpToDate,
    Updated(String),
    /// No install receipt (dev/manual build) or the check failed — open releases.
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
            // axoupdater's blocking API builds its own runtime and downloads +
            // installs inline, so run it off the main thread.
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

/// Check for a newer release and install it if one exists.
fn check_and_install() -> UpdateOutcome {
    let mut updater = AxoUpdater::new_for("rspace");
    // The receipt is written by dist's installer; absent on dev/manual builds.
    if updater.load_receipt().is_err() {
        return UpdateOutcome::Unavailable;
    }
    match updater.is_update_needed_sync() {
        Ok(false) => UpdateOutcome::UpToDate,
        Ok(true) => match updater.run_sync() {
            Ok(Some(result)) => UpdateOutcome::Updated(result.new_version.to_string()),
            Ok(None) => UpdateOutcome::UpToDate,
            Err(_) => UpdateOutcome::Unavailable,
        },
        Err(_) => UpdateOutcome::Unavailable,
    }
}
