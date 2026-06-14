use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::paths::Paths;

/// On-disk storage category surfaced in the Settings view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Config,
    Cache,
    Blobs,
    Logs,
    State,
    /// Files materialized by the OS sync provider (File Provider / CFAPI),
    /// stored outside the app root and sized by the platform crates.
    OsManaged,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryUsage {
    pub category: Category,
    pub path: Option<PathBuf>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageReport {
    pub categories: Vec<CategoryUsage>,
    pub total_bytes: u64,
}

/// Recursively sum file sizes under `path`. Missing paths count as zero.
fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            total += dir_size(&entry.path());
        } else {
            total += meta.len();
        }
    }
    total
}

/// Compute a storage report off the async runtime's worker threads.
///
/// Directory walks can be large; they must never run on the UI thread.
pub async fn report(paths: &Paths) -> StorageReport {
    let dirs = paths.categories();
    tokio::task::spawn_blocking(move || {
        let mut categories = Vec::with_capacity(dirs.len() + 1);
        let mut total = 0;
        for (category, path) in dirs {
            let bytes = dir_size(&path);
            total += bytes;
            categories.push(CategoryUsage { category, path: Some(path), bytes });
        }
        // OS-managed size is filled in by platform crates once mounts exist.
        categories.push(CategoryUsage { category: Category::OsManaged, path: None, bytes: 0 });
        StorageReport { categories, total_bytes: total }
    })
    .await
    .expect("storage sizing task panicked")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn sizes_sum_per_category() {
        let dir = tempdir().unwrap();
        let paths = Paths::with_root(dir.path());
        paths.ensure().unwrap();
        let mut f = std::fs::File::create(paths.cache_dir().join("blob")).unwrap();
        f.write_all(&[0u8; 1024]).unwrap();

        let report = report(&paths).await;
        let cache =
            report.categories.iter().find(|c| c.category == Category::Cache).unwrap();
        assert_eq!(cache.bytes, 1024);
        assert_eq!(report.total_bytes, 1024);
    }
}
