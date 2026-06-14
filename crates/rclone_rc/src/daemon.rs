use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use rand::Rng;
use thiserror::Error;
use tokio::process::{Child, Command};
use tokio::time::{sleep, Instant};

use crate::client::RcClient;

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
/// Lifecycle is leak-proof on three fronts: `kill_on_drop` reaps the child on a
/// normal exit or panic unwind, [`Daemon::shutdown`] does a graceful quit, and a
/// pid file lets the next launch [`reap_orphan`] a daemon left behind by a hard
/// crash where neither of the first two could run.
pub struct Daemon {
    child: Child,
    client: RcClient,
    pidfile: PathBuf,
}

impl Daemon {
    /// Spawn `rclone rcd` on a free loopback port with random credentials.
    ///
    /// Reaps any orphan recorded in `pidfile` first, then writes this daemon's
    /// pid there and waits until it answers `rc/noop`.
    pub async fn start(rclone: &Path, pidfile: PathBuf) -> Result<Self, DaemonError> {
        reap_orphan(&pidfile);

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
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(DaemonError::Spawn)?;

        if let Some(pid) = child.id() {
            write_pidfile(&pidfile, pid).map_err(DaemonError::PidFile)?;
        }

        let client = RcClient::new(format!("http://{addr}"), user, pass);
        let daemon = Self { child, client, pidfile };
        daemon.await_healthy(Duration::from_secs(10)).await?;
        Ok(daemon)
    }

    pub fn client(&self) -> &RcClient {
        &self.client
    }

    async fn await_healthy(&self, timeout: Duration) -> Result<(), DaemonError> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.client.noop().await.is_ok() {
                return Ok(());
            }
            sleep(Duration::from_millis(100)).await;
        }
        Err(DaemonError::Unhealthy(timeout))
    }

    /// Gracefully quit the daemon, falling back to a kill, then clear the pid
    /// file so the next launch has nothing to reap.
    pub async fn shutdown(mut self) {
        let _ = self.client.quit().await;
        let _ = self.child.kill().await;
        let _ = std::fs::remove_file(&self.pidfile);
    }
}

/// Kill a daemon orphaned by a previous run, then clear the pid file.
///
/// Verifies the recorded pid is actually an `rclone rcd` before signalling it,
/// so a pid recycled by an unrelated process is left untouched. Safe to call
/// when no pid file exists (a no-op).
pub fn reap_orphan(pidfile: &Path) {
    let Some(pid) = read_pid(pidfile) else {
        return;
    };
    if is_rclone_rcd(pid) {
        terminate(pid);
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

/// Reserve a free loopback port by binding to `:0` and reading the assignment.
///
/// rclone re-binds the same addr moments later; the gap is a benign race on a
/// single-user loopback interface.
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

/// True only if `pid` names a live process whose command line is `rclone rcd`.
#[cfg(unix)]
fn is_rclone_rcd(pid: u32) -> bool {
    let Ok(out) = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "args="])
        .output()
    else {
        return false;
    };
    let cmd = String::from_utf8_lossy(&out.stdout).to_lowercase();
    cmd.contains("rclone") && cmd.contains("rcd")
}

#[cfg(unix)]
fn terminate(pid: u32) {
    let pid = pid.to_string();
    let _ = std::process::Command::new("kill").args(["-TERM", &pid]).status();
    std::thread::sleep(Duration::from_millis(500));
    if is_alive(&pid) {
        let _ = std::process::Command::new("kill").args(["-KILL", &pid]).status();
    }
}

#[cfg(unix)]
fn is_alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_rclone_rcd(pid: u32) -> bool {
    let Ok(out) = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
    else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout).to_lowercase().contains("rclone")
}

#[cfg(windows)]
fn terminate(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
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
