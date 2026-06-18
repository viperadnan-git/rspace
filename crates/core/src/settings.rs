use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sort::{SortField, SortOrder};

/// User preferences. `#[serde(default)]` keeps partial/old `settings.json`
/// loading, so adding a field is always backward-compatible. Machine-managed
/// layout/state lives in the database ([`crate::db`]), not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Background refresh / stale-revalidate cadence, in seconds.
    pub refresh_secs: u64,
    /// Where downloads go. `None` = the platform default downloads folder.
    pub download_dir: Option<String>,
    /// Explicit rclone binary path. `None` = auto-detect (PATH + known locations).
    pub rclone_path: Option<String>,
    /// Explicit rclone config file (`RCLONE_CONFIG`). `None` = rclone's default.
    pub rclone_config_path: Option<String>,
    /// Default column directory listings are sorted by.
    pub sort_field: SortField,
    /// Default sort direction.
    pub sort_order: SortOrder,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            refresh_secs: 5,
            download_dir: None,
            rclone_path: None,
            rclone_config_path: None,
            sort_field: SortField::Modified,
            sort_order: SortOrder::Desc,
        }
    }
}

impl Settings {
    /// Resolved download directory (configured, or the platform default).
    pub fn download_dir(&self) -> PathBuf {
        self.download_dir.as_ref().map(PathBuf::from).unwrap_or_else(default_download_dir)
    }

    fn read(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)
    }
}

fn default_download_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|u| u.download_dir().map(Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir)
}

/// Owns the settings and their file; [`update`](Self::update) persists on every
/// mutation, so callers never touch the file directly.
pub struct SettingsStore {
    settings: Settings,
    path: PathBuf,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> Self {
        let settings = Settings::read(&path);
        Self { settings, path }
    }

    pub fn get(&self) -> &Settings {
        &self.settings
    }

    /// Mutate and persist in one step.
    pub fn update(&mut self, f: impl FnOnce(&mut Settings)) {
        f(&mut self.settings);
        let _ = self.settings.write(&self.path);
    }
}
