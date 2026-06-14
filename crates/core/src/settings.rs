use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sort::{SortField, SortOrder};

/// User settings. Every field is `#[serde(default)]` via the struct attribute,
/// so partial/old config files load fine — adding a setting can never break an
/// existing `settings.json`. To add one: add a field here with a default in
/// `Default`, then bind a control in the settings UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Background refresh / stale-revalidate cadence, in seconds.
    pub refresh_secs: u64,
    /// Where downloads go. `None` = the platform default downloads folder.
    pub download_dir: Option<String>,
    /// Default column directory listings are sorted by.
    pub sort_field: SortField,
    /// Default sort direction.
    pub sort_order: SortOrder,
    /// Pinned remote names, in display order. Pinned remotes lead the sidebar.
    pub pinned: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            refresh_secs: 5,
            download_dir: None,
            sort_field: SortField::Modified,
            sort_order: SortOrder::Desc,
            pinned: Vec::new(),
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

/// Owns the settings and their file. The single place settings are read and
/// written: every mutation goes through [`update`](Self::update), which persists
/// immediately, so callers never touch the file directly.
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
