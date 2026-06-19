//! Cross-platform process control shared by the daemon and mount reapers:
//! identifying and terminating orphaned `rclone` subprocesses left by a crash.

use std::time::Duration;

/// TERM then (after a grace period) KILL the process.
#[cfg(unix)]
pub(crate) fn terminate(pid: u32) {
    let pid = pid.to_string();
    let _ = std::process::Command::new("kill").args(["-TERM", &pid]).status();
    std::thread::sleep(Duration::from_millis(500));
    if is_alive(&pid) {
        let _ = std::process::Command::new("kill").args(["-KILL", &pid]).status();
    }
}

/// Ask the process to terminate (SIGTERM) so it can clean up — rclone unmounts
/// itself on this, where SIGKILL would orphan a hung NFS mount.
#[cfg(unix)]
pub(crate) fn signal_term(pid: u32) {
    let _ = std::process::Command::new("kill").args(["-TERM", &pid.to_string()]).status();
}

#[cfg(windows)]
pub(crate) fn signal_term(pid: u32) {
    terminate(pid);
}

#[cfg(unix)]
fn is_alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// True if `pid`'s command line contains every `needle` (case-insensitive) —
/// used to confirm a pid is still the rclone process we think it is, never a
/// recycled one, before terminating it.
#[cfg(unix)]
pub(crate) fn cmdline_contains(pid: u32, needles: &[&str]) -> bool {
    let Ok(out) = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "args="])
        .output()
    else {
        return false;
    };
    let cmd = String::from_utf8_lossy(&out.stdout).to_lowercase();
    needles.iter().all(|n| cmd.contains(&n.to_lowercase()))
}

#[cfg(unix)]
pub(crate) fn find_pids(needles: &[&str]) -> Vec<u32> {
    let Ok(out) = std::process::Command::new("ps").args(["-ax", "-o", "pid=,args="]).output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let lc = line.to_lowercase();
            if needles.iter().all(|n| lc.contains(&n.to_lowercase())) {
                line.split_whitespace().next()?.parse().ok()
            } else {
                None
            }
        })
        .collect()
}

#[cfg(windows)]
pub(crate) fn terminate(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
}

#[cfg(windows)]
pub(crate) fn cmdline_contains(pid: u32, needles: &[&str]) -> bool {
    let Ok(out) = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
    else {
        return false;
    };
    let cmd = String::from_utf8_lossy(&out.stdout).to_lowercase();
    needles.iter().all(|n| cmd.contains(&n.to_lowercase()))
}

#[cfg(windows)]
pub(crate) fn find_pids(_needles: &[&str]) -> Vec<u32> {
    Vec::new()
}
