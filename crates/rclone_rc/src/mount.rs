use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
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

/// VFS cache mode for a mount (`--vfs-cache-mode`). `Writes` (the default)
/// streams reads — good for media — while caching writes; `Full` caches reads
/// too (better app compatibility, more disk); `Off`/`Minimal` are near read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheMode {
    Off,
    Minimal,
    Writes,
    Full,
}

impl Default for CacheMode {
    fn default() -> Self {
        Self::Writes
    }
}

impl CacheMode {
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Minimal => "minimal",
            Self::Writes => "writes",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MountConfig {
    pub cache_mode: CacheMode,
    pub read_only: bool,
    /// `--vfs-cache-max-size` (e.g. "10G"); empty = unlimited.
    pub cache_max_size: String,
    /// `--vfs-cache-max-age` (e.g. "1h"); empty = rclone's default.
    pub cache_max_age: String,
}

#[derive(Debug, Error)]
pub enum MountError {
    #[error("spawn rclone nfsmount: {0}")]
    Spawn(std::io::Error),
    #[error("mount point {0} did not appear within {1:?}")]
    Timeout(PathBuf, Duration),
    #[error("{0} is not mounted")]
    NotMounted(String),
}

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
    /// Mount `remote:`, waiting until it is live. The VFS cache (in rclone's
    /// standard per-OS cache dir) makes it writable. macOS uses the built-in NFS
    /// server at `mountpoint` (no macFUSE); Linux mounts there via FUSE; Windows
    /// ignores the path and takes the next free drive letter (`*`). No-op if mounted.
    pub async fn mount(
        &mut self,
        rclone: &Path,
        remote: &str,
        mountpoint: &Path,
        config: &MountConfig,
    ) -> Result<(), MountError> {
        // Short-circuit only while the child is alive; an exited one (crash,
        // external umount, full VFS cache) is a stale entry to replace.
        if let Some(active) = self.active.get_mut(remote) {
            match active.child.try_wait() {
                Ok(None) => return Ok(()),
                _ => {
                    self.active.remove(remote);
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        let _ = std::fs::create_dir_all(mountpoint);

        let mut cmd = Command::new(rclone);
        cmd.arg(MOUNT_CMD).arg(format!("{remote}:"));
        #[cfg(target_os = "windows")]
        cmd.arg("*");
        #[cfg(not(target_os = "windows"))]
        cmd.arg(mountpoint);
        cmd.arg("--vfs-cache-mode").arg(config.cache_mode.as_arg());
        if config.read_only {
            cmd.arg("--read-only");
        }
        if !config.cache_max_size.is_empty() {
            cmd.arg("--vfs-cache-max-size").arg(&config.cache_max_size);
        }
        if !config.cache_max_age.is_empty() {
            cmd.arg("--vfs-cache-max-age").arg(&config.cache_max_age);
        }
        cmd.stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
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

    pub async fn unmount(&mut self, remote: &str) -> Result<(), MountError> {
        let Some(mut m) = self.active.remove(remote) else {
            return Err(MountError::NotMounted(remote.to_string()));
        };
        detach(&mut m).await;
        Ok(())
    }

    pub async fn unmount_all(&mut self) {
        for (_, mut m) in self.active.drain() {
            detach(&mut m).await;
        }
    }
}

/// Stop a mount: SIGTERM lets rclone unmount itself cleanly. SIGKILL (the default
/// `Child::kill`) would orphan a hung NFS mount — server gone, every access blocks
/// — so we never kill before unmounting. Force-unmount backstops an unclean exit.
///
/// The blocking steps run on the blocking pool: a thread parked in a syscall is
/// never polled, which would strand the caller's shutdown timeout.
async fn detach(m: &mut ActiveMount) {
    let mp = m.mountpoint.display().to_string();
    if let Some(pid) = m.child.id() {
        let _ = tokio::task::spawn_blocking(move || crate::proc::signal_term(pid)).await;
        match tokio::time::timeout(Duration::from_secs(5), m.child.wait()).await {
            Ok(_) => tracing::info!(mountpoint = %mp, "unmounted (graceful)"),
            Err(_) => tracing::warn!(mountpoint = %mp, "graceful unmount timed out; forcing"),
        }
    }
    let path = m.mountpoint.clone();
    let _ = tokio::task::spawn_blocking(move || force_unmount(&path)).await;
    let _ = m.child.start_kill();
    let path = m.mountpoint.clone();
    let _ = tokio::task::spawn_blocking(move || remove_mountpoint(&path)).await;
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

#[cfg(unix)]
fn mounted_under(root: &Path) -> Vec<PathBuf> {
    let Ok(out) = std::process::Command::new("mount").output() else {
        return Vec::new();
    };
    let root = root.to_string_lossy().into_owned();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| parse_mountpoint(line, &root))
        .collect()
}

/// The mountpoint under `root` in a `mount` line: `<src> on <mountpoint> (opts)`
/// (macOS) or `<src> on <mountpoint> type <fs> (opts)` (Linux).
///
/// Anchored on `root` and trimmed from the end: rclone permits spaces in remote
/// names, so the first ` on ` / ` type ` is not reliably a field boundary.
#[cfg(unix)]
fn parse_mountpoint(line: &str, root: &str) -> Option<PathBuf> {
    let rest = &line[line.find(root)?..];
    // Options always trail in parens; nothing before them is ours.
    let rest = rest.rfind(" (").map_or(rest, |i| &rest[..i]).trim_end();
    // Linux appends `type <fs>`; an fs type is a single token, so a ` type `
    // followed by more words is part of the remote's name, not the suffix.
    let mountpoint = match rest.rfind(" type ") {
        Some(i) if !rest[i + " type ".len()..].contains(' ') => &rest[..i],
        _ => rest,
    };
    Some(PathBuf::from(mountpoint))
}

#[cfg(not(unix))]
fn mounted_under(_root: &Path) -> Vec<PathBuf> {
    Vec::new()
}

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
    fn parses_mountpoints_with_spaces_in_the_remote_name() {
        let root = "/Users/u/rspace";
        let mac = |l| parse_mountpoint(l, root);
        assert_eq!(mac("srv on /Users/u/rspace/docs (nfs, nodev)").unwrap(), PathBuf::from("/Users/u/rspace/docs"));
        // ` type ` and ` on ` are legal inside an rclone remote name.
        assert_eq!(
            mac("srv on /Users/u/rspace/my type of docs (nfs, nodev)").unwrap(),
            PathBuf::from("/Users/u/rspace/my type of docs")
        );
        assert_eq!(mac("srv on /Users/u/rspace/add on (nfs)").unwrap(), PathBuf::from("/Users/u/rspace/add on"));
        // Linux appends `type <fs>`, a single token, which is stripped.
        assert_eq!(
            mac("srv on /Users/u/rspace/my type of docs type nfs4 (rw)").unwrap(),
            PathBuf::from("/Users/u/rspace/my type of docs")
        );
        // Mounts outside the root are not ours.
        assert!(mac("/dev/disk1 on / (apfs, local)").is_none());
    }

    #[test]
    fn plain_dir_is_not_a_mount() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_mount(dir.path()));
    }
}
