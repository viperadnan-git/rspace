//! App-internal SQLite state, split across two files by lifecycle:
//! `data/rspace.db` (kept: layout state + pinned remotes) and
//! `cache/history.db` (disposable: recent remotes, command usage, job log;
//! wiped by [`Db::clear_history`]). Preferences live in `settings.json`.
//! All access is best-effort — failures swallow and reads default.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// Machine-managed layout state (not user-edited). Persisted as one JSON row.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    pub sidebar_width: Option<f32>,
    /// The right dock's width (shared by all dock panels).
    pub preview_width: Option<f32>,
    pub col_date_width: Option<f32>,
    pub col_size_width: Option<f32>,
}

/// Handle to the app-state databases. Cloneable; clones share the connections.
#[derive(Clone)]
pub struct Db {
    data: Arc<Mutex<Connection>>,
    cache: Arc<Mutex<Connection>>,
}

impl Db {
    /// Open the kept-data and disposable-cache databases, creating them if
    /// needed. Infallible: a file that won't open falls back to in-memory so a
    /// bad disk state degrades history rather than blocking app launch.
    pub fn open(data_path: &Path, cache_path: &Path) -> Self {
        Self {
            data: Arc::new(Mutex::new(open_or_memory(data_path, Self::init_data))),
            cache: Arc::new(Mutex::new(open_or_memory(cache_path, Self::init_cache))),
        }
    }

    fn init_data(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS kv (
                 key   TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS pinned (
                 name     TEXT PRIMARY KEY,
                 position INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS mount_config (
                 name   TEXT PRIMARY KEY,
                 config TEXT NOT NULL
             );",
        )
    }

    fn init_cache(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS recent_remotes (
                 name        TEXT PRIMARY KEY,
                 last_opened INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS command_usage (
                 command   TEXT PRIMARY KEY,
                 count     INTEGER NOT NULL,
                 last_used INTEGER NOT NULL
             );",
        )
    }

    /// Empty the disposable history (recent remotes, command usage).
    ///
    /// VACUUM compacts the db, but in WAL mode it writes the rewrite into the
    /// `-wal` sidecar, which is never truncated while the connection stays open —
    /// so each clean-up actually *grew* the on-disk footprint that the "clearable"
    /// figure measures. The trailing checkpoint folds the WAL back into the db and
    /// truncates it to zero, so the space is genuinely reclaimed.
    pub fn clear_history(&self) {
        let conn = self.cache.lock().unwrap();
        let _ = conn.execute_batch(
            "DELETE FROM recent_remotes; DELETE FROM command_usage;
             VACUUM;
             PRAGMA wal_checkpoint(TRUNCATE);",
        );
    }

    /// Layout state, or the default if nothing has been stored yet.
    pub fn load_ui(&self) -> UiState {
        let conn = self.data.lock().unwrap();
        conn.query_row("SELECT value FROM kv WHERE key = 'ui_state'", [], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_ui(&self, ui: &UiState) {
        let Ok(json) = serde_json::to_string(ui) else {
            return;
        };
        let conn = self.data.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO kv (key, value) VALUES ('ui_state', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [json],
        );
    }

    pub fn load_pinned(&self) -> Vec<String> {
        let conn = self.data.lock().unwrap();
        let Ok(mut stmt) = conn.prepare("SELECT name FROM pinned ORDER BY position") else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) else {
            return Vec::new();
        };
        rows.flatten().collect()
    }

    pub fn save_pinned(&self, names: &[String]) {
        let mut conn = self.data.lock().unwrap();
        let Ok(tx) = conn.transaction() else {
            return;
        };
        let _ = tx.execute("DELETE FROM pinned", []);
        for (i, name) in names.iter().enumerate() {
            let _ = tx.execute("INSERT INTO pinned (name, position) VALUES (?1, ?2)", params![name, i as i64]);
        }
        let _ = tx.commit();
    }

    /// Per-remote mount config as opaque JSON `(name, config)`; the UI owns the
    /// shape. Returned for all remotes so the UI can cache them at startup.
    pub fn load_mount_configs(&self) -> Vec<(String, String)> {
        let conn = self.data.lock().unwrap();
        let Ok(mut stmt) = conn.prepare("SELECT name, config FROM mount_config") else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) else {
            return Vec::new();
        };
        rows.flatten().collect()
    }

    pub fn save_mount_config(&self, name: &str, config: &str) {
        let conn = self.data.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO mount_config (name, config) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET config = excluded.config",
            params![name, config],
        );
    }

    pub fn record_remote(&self, name: &str) {
        let conn = self.cache.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO recent_remotes (name, last_opened) VALUES (?1, unixepoch())
             ON CONFLICT(name) DO UPDATE SET last_opened = unixepoch()",
            params![name],
        );
    }

    pub fn recent_remotes(&self, limit: usize) -> Vec<String> {
        let conn = self.cache.lock().unwrap();
        let Ok(mut stmt) = conn.prepare("SELECT name FROM recent_remotes ORDER BY last_opened DESC LIMIT ?1")
        else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([limit as i64], |r| r.get::<_, String>(0)) else {
            return Vec::new();
        };
        rows.flatten().collect()
    }

    pub fn record_command(&self, command: &str) {
        let conn = self.cache.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO command_usage (command, count, last_used) VALUES (?1, 1, unixepoch())
             ON CONFLICT(command) DO UPDATE SET count = count + 1, last_used = unixepoch()",
            params![command],
        );
    }

    /// Command keys ordered most-used first (count, then recency) — the palette
    /// builds a rank map from this to float used commands up.
    pub fn command_rank(&self) -> Vec<String> {
        let conn = self.cache.lock().unwrap();
        let Ok(mut stmt) =
            conn.prepare("SELECT command FROM command_usage ORDER BY count DESC, last_used DESC")
        else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) else {
            return Vec::new();
        };
        rows.flatten().collect()
    }
}

/// Open a SQLite file (creating its parent), in WAL mode for concurrent reads.
fn open_conn(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path)?;
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    Ok(conn)
}

/// Open + init a SQLite file, falling back to an in-memory database on failure.
fn open_or_memory(path: &Path, init: fn(&Connection) -> rusqlite::Result<()>) -> Connection {
    let opened = open_conn(path).and_then(|c| init(&c).map(|()| c));
    match opened {
        Ok(conn) => conn,
        Err(e) => {
            tracing::error!(path = %path.display(), error = %e, "db open failed; using in-memory");
            let conn = Connection::open_in_memory().expect("in-memory sqlite");
            let _ = init(&conn);
            conn
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Db {
        let data = Connection::open_in_memory().unwrap();
        Db::init_data(&data).unwrap();
        let cache = Connection::open_in_memory().unwrap();
        Db::init_cache(&cache).unwrap();
        Db { data: Arc::new(Mutex::new(data)), cache: Arc::new(Mutex::new(cache)) }
    }

    #[test]
    fn ui_state_defaults_when_absent() {
        let db = memory();
        let ui = db.load_ui();
        assert_eq!(ui.preview_width, None);
        assert_eq!(ui.sidebar_width, None);
    }

    #[test]
    fn ui_state_roundtrips() {
        let db = memory();
        let ui = UiState { sidebar_width: Some(220.0), preview_width: Some(320.0), ..Default::default() };
        db.save_ui(&ui);
        let got = db.load_ui();
        assert_eq!(got.sidebar_width, Some(220.0));
        assert_eq!(got.preview_width, Some(320.0));
    }

    #[test]
    fn recent_remotes_newest_first_and_capped() {
        let db = memory();
        db.record_remote("a");
        db.record_remote("b");
        db.record_remote("a"); // bumps a ahead of b
        let recent = db.recent_remotes(10);
        assert_eq!(recent.first().map(String::as_str), Some("a"));
        assert!(recent.contains(&"b".to_string()));
        assert_eq!(db.recent_remotes(1).len(), 1);
    }

    #[test]
    fn command_rank_orders_by_count() {
        let db = memory();
        db.record_command("Copy");
        db.record_command("Sync");
        db.record_command("Copy");
        let rank = db.command_rank();
        assert_eq!(rank.first().map(String::as_str), Some("Copy"));
    }

    #[test]
    fn clear_history_keeps_pinned_and_ui() {
        let db = memory();
        db.save_pinned(&["gdrive".into()]);
        db.save_ui(&UiState { preview_width: Some(320.0), ..Default::default() });
        db.record_remote("a");
        db.record_command("Copy");
        db.clear_history();
        assert!(db.recent_remotes(10).is_empty());
        assert!(db.command_rank().is_empty());
        assert_eq!(db.load_pinned(), vec!["gdrive".to_string()]);
        assert_eq!(db.load_ui().preview_width, Some(320.0));
    }

    #[test]
    fn pinned_roundtrips_in_order() {
        let db = memory();
        assert!(db.load_pinned().is_empty());
        db.save_pinned(&["gdrive".into(), "s3".into(), "box".into()]);
        assert_eq!(db.load_pinned(), vec!["gdrive".to_string(), "s3".into(), "box".into()]);
        // Replace-all semantics + reordering.
        db.save_pinned(&["s3".into(), "gdrive".into()]);
        assert_eq!(db.load_pinned(), vec!["s3".to_string(), "gdrive".into()]);
    }

    #[test]
    fn save_ui_overwrites() {
        let db = memory();
        db.save_ui(&UiState { preview_width: Some(420.0), ..Default::default() });
        db.save_ui(&UiState { preview_width: Some(300.0), ..Default::default() });
        assert_eq!(db.load_ui().preview_width, Some(300.0));
    }
}
