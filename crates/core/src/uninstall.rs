//! One-shot uninstall: wipe rspace's own state dirs and remove the installed
//! app image. Never touches the user's rclone config or cloud data. Daemon
//! shutdown is handled by the normal quit path that runs afterward.

use std::process::Command;

use crate::Paths;

/// Best-effort teardown: a missing dir or an unremovable running binary is
/// ignored — wiping the state dirs is what matters.
pub fn run(paths: &Paths) {
    purge_state(paths);
    remove_app_image();
}

fn purge_state(paths: &Paths) {
    for dir in paths.state_dirs() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(target_os = "macos")]
fn remove_app_image() {
    let Ok(exe) = std::env::current_exe() else { return };
    // Only when running from a real .app bundle (not `cargo run`).
    let Some(app) = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) else {
        return;
    };
    // Finder "move to trash": recoverable, and safe to relocate a mapped binary.
    let script = format!("tell application \"Finder\" to move POSIX file \"{}\" to trash", app.display());
    let _ = Command::new("osascript").arg("-e").arg(script).status();
}

#[cfg(all(unix, not(target_os = "macos")))]
fn remove_app_image() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe);
    }
}

#[cfg(windows)]
fn remove_app_image() {
    // ponytail: can't delete a running .exe on Windows without a spawned helper;
    // state dirs are already wiped, so leave the binary to the user/installer.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purge_removes_all_state_dirs() {
        let root = std::env::temp_dir().join(format!("rspace-uninstall-{}", std::process::id()));
        let paths = Paths::with_root(&root);
        paths.ensure().unwrap();
        assert!(paths.state_dirs().iter().all(|d| d.exists()));
        purge_state(&paths);
        assert!(paths.state_dirs().iter().all(|d| !d.exists()));
        let _ = std::fs::remove_dir_all(&root);
    }
}
