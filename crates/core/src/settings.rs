use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sort::{SortField, SortOrder};

/// User preferences. `#[serde(default)]` keeps partial/old `settings.json`
/// loading, so adding a field is always backward-compatible. Machine-managed
/// layout/state lives in the database ([`crate::db`]), not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub refresh_secs: u64,
    /// Where downloads go. `None` = the platform default downloads folder.
    pub download_dir: Option<String>,
    /// Explicit rclone binary path. `None` = auto-detect (PATH + known locations).
    pub rclone_path: Option<String>,
    /// Explicit rclone config file (`RCLONE_CONFIG`). `None` = rclone's default.
    pub rclone_config_path: Option<String>,
    pub sort_field: SortField,
    pub sort_order: SortOrder,
    /// Base UI font size in px (Zed's `ui_font_size`); drives the window rem size.
    pub ui_font_size: f32,
    /// Version the user skipped; suppresses the update prompt for just that version.
    pub skipped_update_version: Option<String>,
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
            ui_font_size: 16.0,
            skipped_update_version: None,
        }
    }
}

impl Settings {
    /// Resolved download directory (configured, or the platform default).
    pub fn download_dir(&self) -> PathBuf {
        self.download_dir.as_ref().map(PathBuf::from).unwrap_or_else(default_download_dir)
    }

    fn read(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str(&text) {
            Ok(settings) => settings,
            // Keep the original: the next `update` would overwrite it with
            // defaults, making an unreadable file permanent loss.
            Err(e) => {
                tracing::error!(path = %path.display(), error = %e, "unreadable settings; using defaults");
                let _ = std::fs::rename(path, path.with_extension("json.bak"));
                Self::default()
            }
        }
    }

    /// Write atomically; a truncate-in-place cut short leaves half a file.
    fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path)
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

    pub fn update(&mut self, f: impl FnOnce(&mut Settings)) {
        f(&mut self.settings);
        let _ = self.settings.write(&self.path);
    }
}

#[cfg(test)]
mod durability_tests {
    use super::*;

    /// A unique scratch dir; `core` has no tempfile dev-dep (cf. uninstall.rs).
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("rspace-settings-{}-{tag}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn corrupt_settings_are_preserved_not_overwritten() {
        let dir = scratch("corrupt");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ truncated").unwrap();

        let s = Settings::read(&path);
        assert_eq!(s.rclone_path, None, "falls back to defaults");
        let backup = path.with_extension("json.bak");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{ truncated", "original kept");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_round_trips_and_leaves_no_temp() {
        let dir = scratch("write");
        let path = dir.join("settings.json");
        let s = Settings { rclone_path: Some("/usr/local/bin/rclone".into()), ..Settings::default() };
        s.write(&path).unwrap();

        assert_eq!(Settings::read(&path).rclone_path.as_deref(), Some("/usr/local/bin/rclone"));
        assert!(!path.with_extension("json.tmp").exists(), "temp file is renamed away");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
