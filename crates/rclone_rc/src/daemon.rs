use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use thiserror::Error;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, Instant};

use crate::client::RcClient;
#[cfg(any(unix, windows))]
use crate::mount::SharedMounts;

/// Shared, restartable handle to the daemon (held by the [`Service`] and the
/// signal handler so both always act on the current child).
///
/// [`Service`]: crate::Service
pub type SharedDaemon = Arc<Mutex<Daemon>>;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("spawn rclone rcd: {0}")]
    Spawn(std::io::Error),
    #[error("no free loopback port: {0}")]
    Port(std::io::Error),
    #[error("write pid file: {0}")]
    PidFile(std::io::Error),
    #[error("daemon did not become healthy within {0:?}")]
    Unhealthy(Duration),
}

/// A running `rclone rcd` instance bound to loopback with token auth.
///
/// Leak-proof via `kill_on_drop` (normal exit/unwind), [`Daemon::shutdown`]
/// (graceful quit), signal cleanup, and a pid file [`reap_orphan`]ed on the next
/// launch after a hard crash.
pub struct Daemon {
    child: Child,
    client: RcClient,
    pidfile: PathBuf,
    rclone: PathBuf,
}

impl Daemon {
    /// Spawn `rclone rcd` on a free loopback port with random credentials.
    ///
    /// Reaps any orphan recorded in `pidfile` first, then writes this daemon's
    /// pid there and waits until it answers `rc/noop`.
    pub async fn start(rclone: PathBuf, pidfile: PathBuf) -> Result<Self, DaemonError> {
        reap_orphan(&pidfile);
        let (child, client) = spawn_healthy(&rclone, &pidfile).await?;
        Ok(Self { child, client, pidfile, rclone })
    }

    pub fn client(&self) -> &RcClient {
        &self.client
    }

    /// Stop the current daemon and spawn a fresh one on a new port, returning the
    /// new client. The new endpoint differs, so callers must adopt the returned
    /// client (the [`Service`] swaps it for all clones).
    ///
    /// [`Service`]: crate::Service
    pub async fn restart(&mut self) -> Result<RcClient, DaemonError> {
        let _ = self.client.quit().await;
        let _ = self.child.kill().await;
        let (child, client) = spawn_healthy(&self.rclone, &self.pidfile).await?;
        self.child = child;
        self.client = client.clone();
        Ok(client)
    }

    /// Gracefully quit the daemon, falling back to a kill, then clear the pid
    /// file so the next launch has nothing to reap. Takes `&mut self` so it can
    /// run while the daemon is held behind a [`SharedDaemon`].
    pub async fn shutdown(&mut self) {
        let _ = tokio::time::timeout(Duration::from_secs(2), self.client.quit()).await;
        let _ = self.child.kill().await;
        let _ = std::fs::remove_file(&self.pidfile);
    }
}

/// Spawn `rclone rcd` on a fresh free port with random credentials, record its
/// pid, and wait until it answers `rc/noop`. Shared by `start` and `restart`.
async fn spawn_healthy(rclone: &Path, pidfile: &Path) -> Result<(Child, RcClient), DaemonError> {
    let port = free_loopback_port().map_err(DaemonError::Port)?;
    let addr = format!("127.0.0.1:{port}");
    let user = token(16);
    let pass = token(32);

    let child = Command::new(rclone)
        .arg("rcd")
        .arg("--rc-addr")
        .arg(&addr)
        .arg("--rc-user")
        .arg(&user)
        .arg("--rc-pass")
        .arg(&pass)
        // Serve remote objects over the same HTTP endpoint for file previews.
        .arg("--rc-serve")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(DaemonError::Spawn)?;

    if let Some(pid) = child.id() {
        write_pidfile(pidfile, pid).map_err(DaemonError::PidFile)?;
    }

    let client = RcClient::new(format!("http://{addr}"), user, pass);
    await_healthy(&client, Duration::from_secs(10)).await?;
    Ok((child, client))
}

async fn await_healthy(client: &RcClient, timeout: Duration) -> Result<(), DaemonError> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if client.noop().await.is_ok() {
            return Ok(());
        }
        sleep(Duration::from_millis(100)).await;
    }
    Err(DaemonError::Unhealthy(timeout))
}

/// Tear down mounts and the daemon on a termination signal, then exit —
/// `kill_on_drop` doesn't run when a signal kills the process. Acts on the
/// shared (restart-safe) daemon. Unix: SIGINT/SIGTERM. Windows: Ctrl-* events.
#[cfg(unix)]
pub fn install_signal_cleanup(
    handle: &tokio::runtime::Handle,
    daemon: SharedDaemon,
    mounts: SharedMounts,
) {
    use tokio::signal::unix::{signal, SignalKind};

    handle.spawn(async move {
        let (Ok(mut term), Ok(mut int)) =
            (signal(SignalKind::terminate()), signal(SignalKind::interrupt()))
        else {
            return;
        };
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
        }
        signal_shutdown(daemon, mounts).await;
    });
}

#[cfg(windows)]
pub fn install_signal_cleanup(
    handle: &tokio::runtime::Handle,
    daemon: SharedDaemon,
    mounts: SharedMounts,
) {
    use tokio::signal::windows;

    handle.spawn(async move {
        let (Ok(mut cc), Ok(mut brk), Ok(mut close), Ok(mut shutdown), Ok(mut logoff)) = (
            windows::ctrl_c(),
            windows::ctrl_break(),
            windows::ctrl_close(),
            windows::ctrl_shutdown(),
            windows::ctrl_logoff(),
        ) else {
            return;
        };
        tokio::select! {
            _ = cc.recv() => {}
            _ = brk.recv() => {}
            _ = close.recv() => {}
            _ = shutdown.recv() => {}
            _ = logoff.recv() => {}
        }
        signal_shutdown(daemon, mounts).await;
    });
}

/// Unmount everything and shut the daemon down (bounded), then exit. A stuck
/// lock (mid restart) is bounded too — the pid file and mount-table reap on the
/// next launch are the backstops for any survivor.
#[cfg(any(unix, windows))]
async fn signal_shutdown(daemon: SharedDaemon, mounts: SharedMounts) -> ! {
    let _ = tokio::time::timeout(Duration::from_secs(4), async {
        mounts.lock().await.unmount_all().await;
        daemon.lock().await.shutdown().await;
    })
    .await;
    std::process::exit(130);
}

/// Kill a daemon orphaned by a previous run, then clear the pid file. The pid is
/// verified as an `rclone rcd` first, so a recycled pid is left untouched.
pub fn reap_orphan(pidfile: &Path) {
    let Some(pid) = read_pid(pidfile) else {
        return;
    };
    if crate::proc::cmdline_contains(pid, &["rclone", "rcd"]) {
        crate::proc::terminate(pid);
    }
    let _ = std::fs::remove_file(pidfile);
}

fn read_pid(pidfile: &Path) -> Option<u32> {
    std::fs::read_to_string(pidfile).ok()?.trim().parse().ok()
}

fn write_pidfile(path: &Path, pid: u32) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, pid.to_string())
}

/// Reserve a free loopback port via `:0`. rclone re-binds it moments later — a
/// benign race on a single-user loopback interface.
fn free_loopback_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn token(len: usize) -> String {
    const CHARSET: &[u8] =
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..len).map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_has_requested_length() {
        assert_eq!(token(24).len(), 24);
    }

    #[test]
    fn free_port_is_nonzero() {
        assert!(free_loopback_port().unwrap() > 0);
    }

    #[test]
    fn reap_orphan_is_noop_without_pidfile() {
        let dir = tempfile::tempdir().unwrap();
        reap_orphan(&dir.path().join("absent.pid"));
    }

    #[test]
    fn reap_orphan_clears_stale_pidfile() {
        // A pid that is not our rclone (use our own pid: alive but not rclone).
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("rcd.pid");
        write_pidfile(&pidfile, std::process::id()).unwrap();
        reap_orphan(&pidfile);
        assert!(!pidfile.exists());
    }
}
