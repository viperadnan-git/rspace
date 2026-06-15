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

    pub fn config_dir(&self) -> PathBuf {
        self.root.join("config")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state")
    }

    pub fn settings_path(&self) -> PathBuf {
        self.config_dir().join("settings.json")
    }

    /// Create the root and all subdirectories if missing.
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [self.config_dir(), self.logs_dir(), self.state_dir()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
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
