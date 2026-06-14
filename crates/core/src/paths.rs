use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use thiserror::Error;

use crate::accounting::Category;

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
/// Everything here is removed wholesale on uninstall. Anything created *outside*
/// it (OS-managed storage) is tracked separately in the teardown manifest.
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

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn blobs_dir(&self) -> PathBuf {
        self.root.join("blobs")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state")
    }

    /// Teardown manifest path (records OS-managed artifacts outside the root).
    pub fn manifest_path(&self) -> PathBuf {
        self.state_dir().join("teardown.jsonl")
    }

    pub fn settings_path(&self) -> PathBuf {
        self.config_dir().join("settings.json")
    }

    /// Create the root and all subdirectories if missing.
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [
            self.config_dir(),
            self.cache_dir(),
            self.blobs_dir(),
            self.logs_dir(),
            self.state_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// Managed subdirectories paired with their storage category.
    pub fn categories(&self) -> [(Category, PathBuf); 5] {
        [
            (Category::Config, self.config_dir()),
            (Category::Cache, self.cache_dir()),
            (Category::Blobs, self.blobs_dir()),
            (Category::Logs, self.logs_dir()),
            (Category::State, self.state_dir()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdirs_live_under_root() {
        let p = Paths::with_root("/tmp/rspace-test");
        assert!(p.config_dir().starts_with(p.root()));
        assert_eq!(p.manifest_path(), p.state_dir().join("teardown.jsonl"));
    }
}
