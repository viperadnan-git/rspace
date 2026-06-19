use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

pub const INSTALL_URL: &str = "https://rclone.org/install/";

const BIN: &str = if cfg!(windows) { "rclone.exe" } else { "rclone" };

#[derive(Debug, Clone)]
pub struct Rclone {
    pub path: PathBuf,
    pub version: String,
}

#[derive(Debug, Error)]
#[error("rclone not found; install it from {INSTALL_URL}")]
pub struct NotFound;

/// Locate the rclone binary: `PATH` first, then well-known install locations.
///
/// A GUI app launched from Finder/Dock inherits a minimal `PATH`, so the
/// fallback locations matter even when rclone is on the user's shell `PATH`.
pub fn detect() -> Result<Rclone, NotFound> {
    let path = path_lookup().or_else(known_locations).ok_or(NotFound)?;
    let version = version(&path).ok_or(NotFound)?;
    Ok(Rclone { path, version })
}

/// Validate a user-supplied rclone path (e.g. from settings): returns its
/// version, or `None` when it isn't a working rclone binary.
pub fn from_path(path: impl Into<PathBuf>) -> Option<Rclone> {
    let path = path.into();
    let version = version(&path)?;
    Some(Rclone { path, version })
}

fn path_lookup() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(BIN))
        .find(|candidate| candidate.is_file())
}

fn known_locations() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        dirs.push("/opt/homebrew/bin".into()); // Apple-silicon Homebrew
        dirs.push("/usr/local/bin".into()); // Intel Homebrew
    }
    if cfg!(unix) {
        dirs.push("/usr/bin".into());
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(Path::new(&home).join(".local/bin"));
        }
    }
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(Path::new(&local).join("Microsoft\\WinGet\\Links"));
        }
    }
    dirs.into_iter().map(|d| d.join(BIN)).find(|c| c.is_file())
}

fn version(path: &Path) -> Option<String> {
    let output = Command::new(path).arg("version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(|l| l.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_name_matches_platform() {
        if cfg!(windows) {
            assert_eq!(BIN, "rclone.exe");
        } else {
            assert_eq!(BIN, "rclone");
        }
    }
}
