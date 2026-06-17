use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use thiserror::Error;

/// Reverse-DNS application identifier; also the macOS bundle id.
pub const APP_QUALIFIER: &str = "com";
pub const APP_ORG: &str = "viperadnan";
pub const APP_NAME: &str = "rspace";

#[derive(Debug, Error)]
pub enum PathsError {
    #[error("could not determine a home directory for the current user")]
    NoHome,
}

/// Single on-disk root for all rspace state, with typed subdirectories.
///
/// To add a subdirectory: add a `*_dir()` accessor and list it in
/// [`ensure`](Self::ensure) so it is created on startup.
#[derive(Debug, Clone)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    /// Resolve the app root from OS conventions
    /// (App Support / LOCALAPPDATA / XDG data dir).
    pub fn resolve() -> Result<Self, PathsError> {
        let dirs =
            ProjectDirs::from(APP_QUALIFIER, APP_ORG, APP_NAME).ok_or(PathsError::NoHome)?;
        Ok(Self { root: dirs.data_dir().to_path_buf() })
    }

    /// Build paths under an explicit root (tests, portable mode).
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Preferences (kept). User-facing, never auto-cleaned.
    pub fn config_dir(&self) -> PathBuf {
        self.root.join("config")
    }

    /// Persistent app data (kept): pinned remotes, window layout.
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    /// Disposable data — everything the Clean-up action wipes (history, logs).
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.cache_dir().join("logs")
    }

    pub fn settings_path(&self) -> PathBuf {
        self.config_dir().join("settings.json")
    }

    /// Kept app state: pinned remotes + window layout.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir().join("rspace.db")
    }

    /// Disposable history: recent remotes, command usage, job log.
    pub fn history_db_path(&self) -> PathBuf {
        self.cache_dir().join("history.db")
    }

    /// Running-daemon pid (runtime; left untouched by clean-up).
    pub fn pid_path(&self) -> PathBuf {
        self.root.join("rcd.pid")
    }

    /// Create the root and all subdirectories if missing.
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [self.config_dir(), self.data_dir(), self.logs_dir()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// Total size in bytes of all files under `path` (recursive; missing → 0).
pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdirs_live_under_root() {
        let p = Paths::with_root("/tmp/rspace-test");
        assert!(p.config_dir().starts_with(p.root()));
        assert_eq!(p.settings_path(), p.config_dir().join("settings.json"));
    }
}
