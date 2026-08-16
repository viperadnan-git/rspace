use std::path::{Path, PathBuf};

use directories::{BaseDirs, UserDirs};
use thiserror::Error;

/// User-visible root for mountpoints (`~/rspace`); each remote mounts at
/// `mount_root()/<remote>`. Separate from the app's state directories.
pub fn mount_root() -> Option<PathBuf> {
    UserDirs::new().map(|u| u.home_dir().join(APP_NAME))
}

pub const APP_NAME: &str = "rspace";

#[derive(Debug, Error)]
pub enum PathsError {
    #[error("could not determine a home directory for the current user")]
    NoHome,
}

/// Per-category state directories, each resolved by the OS convention. Modeled on
/// Zed: config is XDG-style even on macOS (`~/.config/rspace`), data/cache/logs
/// stay platform-native there.
#[derive(Debug, Clone)]
pub struct Paths {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    logs: PathBuf,
}

impl Paths {
    pub fn resolve() -> Result<Self, PathsError> {
        let dirs = BaseDirs::new().ok_or(PathsError::NoHome)?;
        let home = dirs.home_dir();
        // XDG `~/.config/rspace` everywhere (rclone-style), not the platform config
        // dir; Linux still honours `$XDG_CONFIG_HOME` via the resolver.
        let config = if cfg!(any(target_os = "linux", target_os = "freebsd")) {
            dirs.config_dir().join(APP_NAME)
        } else {
            home.join(".config").join(APP_NAME)
        };
        let data = dirs.data_local_dir().join(APP_NAME);
        let cache = dirs.cache_dir().join(APP_NAME);
        let logs = if cfg!(target_os = "macos") {
            home.join("Library/Logs").join(APP_NAME)
        } else {
            data.join("logs")
        };
        Ok(Self { config, data, cache, logs })
    }

    /// All categories under one root (tests, portable mode).
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
            logs: root.join("logs"),
        }
    }

    /// Preferences (kept). User-facing, never auto-cleaned.
    pub fn config_dir(&self) -> &Path {
        &self.config
    }

    /// Persistent app data (kept): pinned remotes, window layout.
    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    /// Disposable data: recent history and the daemon pid.
    pub fn cache_dir(&self) -> &Path {
        &self.cache
    }

    pub fn logs_dir(&self) -> &Path {
        &self.logs
    }

    pub fn settings_path(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    /// Kept app state: pinned remotes + window layout.
    pub fn db_path(&self) -> PathBuf {
        self.data.join("rspace.db")
    }

    /// Disposable history: recent remotes, command usage, job log.
    pub fn history_db_path(&self) -> PathBuf {
        self.cache.join("history.db")
    }

    /// This instance's daemon pidfile, tagged with the owning process id so a
    /// second instance can tell a crashed run's orphan from a live sibling's.
    pub fn pid_path(&self) -> PathBuf {
        self.cache.join(format!("rcd-{}.pid", std::process::id()))
    }

    /// The four state dirs (config, data, cache, logs) — for bulk create/wipe.
    pub fn state_dirs(&self) -> [&Path; 4] {
        [&self.config, &self.data, &self.cache, &self.logs]
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in self.state_dirs() {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// `(total, clearable)` bytes. `total` counts each root once (logs nest under
    /// data on Linux; data and cache are the same `%LOCALAPPDATA%` on Windows).
    ///
    /// `clearable` counts the artifacts clean-up removes — the history db and the
    /// log dir — not whole roots, which can coincide with a kept one.
    pub fn storage_size(&self) -> (u64, u64) {
        let total = sum_distinct(&[&self.config, &self.data, &self.cache, &self.logs]);
        let history: u64 = {
            let db = self.history_db_path();
            let db = db.to_string_lossy();
            // SQLite keeps its WAL and shared-memory files beside the db.
            ["", "-wal", "-shm"]
                .iter()
                .filter_map(|ext| std::fs::metadata(format!("{db}{ext}")).ok())
                .map(|m| m.len())
                .sum()
        };
        (total, history + dir_size(&self.logs))
    }
}

/// Sum `dir_size`, skipping a dir that duplicates or nests under another in the
/// list (so shared/nested roots aren't double-counted).
fn sum_distinct(dirs: &[&PathBuf]) -> u64 {
    let mut roots: Vec<&PathBuf> = Vec::new();
    for d in dirs {
        if roots.iter().any(|r| d.starts_with(r)) {
            continue;
        }
        roots.retain(|r| !r.starts_with(d));
        roots.push(d);
    }
    roots.iter().map(|r| dir_size(r)).sum()
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
        assert!(p.config_dir().starts_with("/tmp/rspace-test"));
        assert_eq!(p.settings_path(), p.config_dir().join("settings.json"));
        // Tagged with this process's id, so two instances never share a pidfile.
        assert_eq!(p.pid_path(), p.cache_dir().join(format!("rcd-{}.pid", std::process::id())));
    }
}
