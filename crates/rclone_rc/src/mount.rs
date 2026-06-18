use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, Instant};

/// Shared handle to the live mounts (held by the [`Service`] and the signal
/// handler so both tear them down).
///
/// [`Service`]: crate::Service
pub type SharedMounts = Arc<Mutex<Mounts>>;

/// rclone mount subcommand per platform. macOS uses the built-in NFS server
/// (no macFUSE); Linux mounts via FUSE (libfuse is normally preinstalled);
/// Windows via WinFsp — all through `mount`, except macOS's `nfsmount`.
#[cfg(target_os = "macos")]
const MOUNT_CMD: &str = "nfsmount";
#[cfg(not(target_os = "macos"))]
const MOUNT_CMD: &str = "mount";

#[derive(Debug, Error)]
pub enum MountError {
    #[error("spawn rclone nfsmount: {0}")]
    Spawn(std::io::Error),
    #[error("mount point {0} did not appear within {1:?}")]
    Timeout(PathBuf, Duration),
    #[error("{0} is not mounted")]
    NotMounted(String),
}

/// One live `rclone nfsmount` process and where it is mounted.
struct ActiveMount {
    mountpoint: PathBuf,
    child: Child,
}

/// Tracks no-install NFS mounts (`rclone nfsmount`), keyed by remote name.
///
/// macOS mounts through the built-in NFS client — no macFUSE, no sudo. Killing
/// the child cleanly unmounts (rclone tears the mount down on exit);
/// `kill_on_drop` is the backstop for an unclean app exit.
#[derive(Default)]
pub struct Mounts {
    active: HashMap<String, ActiveMount>,
}

impl Mounts {
    /// Mount `remote:`, waiting until it is live. A VFS write cache (under
    /// `cache_dir`) makes it writable. macOS uses the built-in NFS server at
    /// `mountpoint` (no macFUSE); Linux mounts there via FUSE; Windows ignores
    /// the path and takes the next free drive letter (`*`). No-op if mounted.
    pub async fn mount(
        &mut self,
        rclone: &Path,
        cache_dir: &Path,
        remote: &str,
        mountpoint: &Path,
    ) -> Result<(), MountError> {
        if self.active.contains_key(remote) {
            return Ok(());
        }
        let _ = std::fs::create_dir_all(cache_dir);
        #[cfg(not(target_os = "windows"))]
        let _ = std::fs::create_dir_all(mountpoint);

        let mut cmd = Command::new(rclone);
        cmd.arg(MOUNT_CMD).arg(format!("{remote}:"));
        #[cfg(target_os = "windows")]
        cmd.arg("*");
        #[cfg(not(target_os = "windows"))]
        cmd.arg(mountpoint);
        cmd.arg("--vfs-cache-mode")
            .arg("writes")
            .arg("--cache-dir")
            .arg(cache_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let child = cmd.spawn().map_err(MountError::Spawn)?;

        if let Err(e) = await_mounted(mountpoint, Duration::from_secs(15)).await {
            // Never came up: tear down gracefully (SIGTERM — not kill_on_drop's
            // SIGKILL, which would orphan a partial NFS mount) and drop the dir.
            detach(&mut ActiveMount { mountpoint: mountpoint.to_path_buf(), child }).await;
            return Err(e);
        }
        tracing::info!(remote, "mounted");
        self.active
            .insert(remote.to_string(), ActiveMount { mountpoint: mountpoint.to_path_buf(), child });
        Ok(())
    }

    /// Unmount `remote`.
    pub async fn unmount(&mut self, remote: &str) -> Result<(), MountError> {
        let Some(mut m) = self.active.remove(remote) else {
            return Err(MountError::NotMounted(remote.to_string()));
        };
        detach(&mut m).await;
        Ok(())
    }

    /// Unmount everything (on app shutdown).
    pub async fn unmount_all(&mut self) {
        for (_, mut m) in self.active.drain() {
            detach(&mut m).await;
        }
    }
}

/// Stop a mount: SIGTERM lets rclone unmount itself cleanly. SIGKILL (the
/// default `Child::kill`) would orphan a hung NFS mount — server gone, every
/// access blocks — so we never kill before unmounting, nor stat the mount after.
/// Force-unmount is the backstop for an unclean exit.
async fn detach(m: &mut ActiveMount) {
    let mp = m.mountpoint.display().to_string();
    if let Some(pid) = m.child.id() {
        crate::proc::signal_term(pid);
        match tokio::time::timeout(Duration::from_secs(5), m.child.wait()).await {
            Ok(_) => tracing::info!(mountpoint = %mp, "unmounted (graceful)"),
            Err(_) => tracing::warn!(mountpoint = %mp, "graceful unmount timed out; forcing"),
        }
    }
    force_unmount(&m.mountpoint);
    let _ = m.child.start_kill();
    remove_mountpoint(&m.mountpoint);
}

/// Remove the now-idle mountpoint, and the mount root if it was the last one,
/// so an unmounted remote leaves no stray folder. Bails if it is still a mount
/// (its contents are the remote — never to be touched); otherwise clears the
/// `.DS_Store` Finder leaves behind and removes the empty dir.
fn remove_mountpoint(mountpoint: &Path) {
    if is_mount(mountpoint) {
        return;
    }
    let _ = std::fs::remove_file(mountpoint.join(".DS_Store"));
    let _ = std::fs::remove_dir(mountpoint);
    if let Some(parent) = mountpoint.parent() {
        let _ = std::fs::remove_file(parent.join(".DS_Store"));
        let _ = std::fs::remove_dir(parent);
    }
}

/// Reap mounts left by a crashed run: force-unmount any mount under `mount_root`
/// and kill the orphaned `rclone nfsmount` serving it. The mount table plus a
/// known root are the source of truth, so there is no persisted state to drift.
pub fn reap_orphans(mount_root: &Path) {
    for mp in mounted_under(mount_root) {
        tracing::warn!(mountpoint = %mp.display(), "reaping mount left by a previous run");
        // Detach the (possibly dead-server) mount first, then kill any survivor.
        force_unmount(&mp);
        for pid in crate::proc::find_pids(&["rclone", MOUNT_CMD, &mp.to_string_lossy()]) {
            crate::proc::terminate(pid);
        }
        remove_mountpoint(&mp);
    }
}

/// Mountpoints currently mounted under `root`, read from the OS mount table.
#[cfg(unix)]
fn mounted_under(root: &Path) -> Vec<PathBuf> {
    let Ok(out) = std::process::Command::new("mount").output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(parse_mountpoint)
        .filter(|mp| mp.starts_with(root))
        .collect()
}

/// The mountpoint in a `mount` line: `<src> on <mountpoint> (opts)` (macOS) or
/// `<src> on <mountpoint> type <fs> (opts)` (Linux).
#[cfg(unix)]
fn parse_mountpoint(line: &str) -> Option<PathBuf> {
    let after = line.split(" on ").nth(1)?;
    let end = after.find(" type ").or_else(|| after.find(" (")).unwrap_or(after.len());
    Some(PathBuf::from(&after[..end]))
}

#[cfg(not(unix))]
fn mounted_under(_root: &Path) -> Vec<PathBuf> {
    Vec::new()
}

/// Poll until `mountpoint` becomes a mount (a path mount, macOS/Linux).
#[cfg(unix)]
async fn await_mounted(mountpoint: &Path, timeout: Duration) -> Result<(), MountError> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if is_mount(mountpoint) {
            return Ok(());
        }
        sleep(Duration::from_millis(200)).await;
    }
    Err(MountError::Timeout(mountpoint.to_path_buf(), timeout))
}

/// Windows assigns the drive letter itself; just give it a moment to appear.
#[cfg(not(unix))]
async fn await_mounted(_mountpoint: &Path, _timeout: Duration) -> Result<(), MountError> {
    sleep(Duration::from_millis(1500)).await;
    Ok(())
}

/// A mount root sits on a different device than the directory it covers.
#[cfg(unix)]
fn is_mount(mountpoint: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let parent = mountpoint.parent().unwrap_or(mountpoint);
    match (std::fs::metadata(mountpoint), std::fs::metadata(parent)) {
        (Ok(here), Ok(up)) => here.dev() != up.dev(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn is_mount(_mountpoint: &Path) -> bool {
    false
}

/// Force-unmount as a backstop after SIGTERM. macOS: `umount -f` detaches a hung
/// NFS mount without hanging (`diskutil unmount` blocks when the server is gone).
#[cfg(target_os = "macos")]
fn force_unmount(mountpoint: &Path) {
    let _ = std::process::Command::new("umount")
        .arg("-f")
        .arg(mountpoint)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Linux: FUSE mounts unmount via `fusermount` (`-z` lazy, so a busy mount
/// detaches once free).
#[cfg(target_os = "linux")]
fn force_unmount(mountpoint: &Path) {
    let _ = std::process::Command::new("fusermount")
        .arg("-uz")
        .arg(mountpoint)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Windows: killing the rclone process releases the drive letter (WinFsp).
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn force_unmount(_mountpoint: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_dir_is_not_a_mount() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_mount(dir.path()));
    }
}
