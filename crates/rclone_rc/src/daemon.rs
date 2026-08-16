use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, Instant};

use crate::client::RcClient;
#[cfg(any(unix, windows))]
use crate::mount::SharedMounts;

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
        // Ctrl-C in a terminal delivers SIGINT to the whole process group, so the
        // daemon often self-exits before we get here; skip `core/quit` only when
        // it has definitely exited (else it'd fail to connect and log a warning).
        // An indeterminate `Err` still tries quit rather than risk a survivor.
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = tokio::time::timeout(Duration::from_secs(2), self.client.quit()).await;
        }
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
        // Credentials go through the environment, never argv: a process command
        // line is world-readable (`ps -ax -o args=` — see proc.rs), and with
        // --rc-serve below these grant read/write on every configured remote.
        .env("RCLONE_RC_USER", &user)
        .env("RCLONE_RC_PASS", &pass)
        // Serve remote objects over the same HTTP endpoint for file previews.
        .arg("--rc-serve")
        // The daemon is the source of truth for tasks; keep finished jobs for the
        // session so the Tasks panel can show them (rclone expires them in 60s by
        // default).
        .arg("--rc-job-expire-duration")
        .arg("24h")
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

/// Upper bound on teardown (unmounts + daemon exit) before the process gives up
/// and exits anyway. Shared by the signal path and [`crate::Service::shutdown`].
pub(crate) const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(4);

/// Unmount everything and shut the daemon down (bounded), then exit. A stuck
/// lock (mid restart) is bounded too — the pid file and mount-table reap on the
/// next launch are the backstops for any survivor.
#[cfg(any(unix, windows))]
async fn signal_shutdown(daemon: SharedDaemon, mounts: SharedMounts) -> ! {
    let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, async {
        mounts.lock().await.unmount_all().await;
        daemon.lock().await.shutdown().await;
    })
    .await;
    std::process::exit(130);
}

/// Kill daemons orphaned by crashed runs: every `rcd-<owner>.pid` beside
/// `pidfile` whose owning process is gone. A live instance's daemon is left
/// alone. Each pid is verified as an `rclone rcd` before signalling, so a
/// recycled pid is untouched.
pub fn reap_orphan(pidfile: &Path) {
    let (Some(dir), me) = (pidfile.parent(), std::process::id()) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|e| e.path()) {
        let Some(owner) = pid_owner(&path) else {
            continue;
        };
        if owner == me || crate::proc::is_running(owner) {
            continue;
        }
        if let Some(pid) = read_pid(&path)
            && crate::proc::cmdline_contains(pid, &["rclone", "rcd"])
        {
            crate::proc::terminate(pid);
        }
        let _ = std::fs::remove_file(&path);
    }
}

/// Owner process id encoded in an `rcd-<owner>.pid` file name.
fn pid_owner(path: &Path) -> Option<u32> {
    path.file_name()?.to_str()?.strip_prefix("rcd-")?.strip_suffix(".pid")?.parse().ok()
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

/// A random hex token of exactly `len` chars for the daemon's rc auth.
fn token(len: usize) -> String {
    let mut bytes = vec![0u8; len.div_ceil(2)];
    getrandom::fill(&mut bytes).expect("system RNG unavailable");
    let mut s: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    s.truncate(len);
    s
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
    fn reap_orphan_clears_a_dead_owners_pidfile() {
        // Owner pid 1 is init, never an rspace instance, so this file is stale.
        // (The recorded pid is our own: alive, but not an `rclone rcd`, so it is
        // verified and left unsignalled — only the file goes.)
        let dir = tempfile::tempdir().unwrap();
        let ours = dir.path().join(format!("rcd-{}.pid", std::process::id()));
        let stale = dir.path().join("rcd-999999999.pid");
        write_pidfile(&stale, std::process::id()).unwrap();
        write_pidfile(&ours, std::process::id()).unwrap();
        reap_orphan(&ours);
        assert!(!stale.exists(), "a dead owner's pidfile is reaped");
        assert!(ours.exists(), "our own pidfile survives");
    }

    #[test]
    fn reap_orphan_spares_a_live_siblings_pidfile() {
        // The owner is alive (this test process stands in for a second
        // instance), so its daemon must not be touched.
        let dir = tempfile::tempdir().unwrap();
        let sibling = dir.path().join(format!("rcd-{}.pid", std::process::id()));
        write_pidfile(&sibling, std::process::id()).unwrap();
        reap_orphan(&dir.path().join("rcd-1.pid"));
        assert!(sibling.exists());
    }
}
