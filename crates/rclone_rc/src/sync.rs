//! Compare two filesystems via rclone's own `operations/check` (authoritative —
//! the same size/hash logic a real sync uses), parsed into a flat per-file diff.

use serde_json::Value;

/// How to reconcile two folders. `Copy` and `Mirror` are one-way (source →
/// destination); `Bisync` is bidirectional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// Add/update on the destination, never delete (`sync/copy`).
    Copy,
    /// Make the destination match the source, deleting extras (`sync/sync`).
    Mirror,
    /// Two-way reconcile with conflict handling (`sync/bisync`).
    Bisync,
}

impl SyncMode {
    pub fn label(self) -> &'static str {
        match self {
            SyncMode::Copy => "Copy",
            // rclone's own name for the mirror operation (`rclone sync`).
            SyncMode::Mirror => "Sync",
            SyncMode::Bisync => "Bisync",
        }
    }

    pub fn cli_verb(self) -> &'static str {
        match self {
            SyncMode::Copy => "copy",
            SyncMode::Mirror => "sync",
            SyncMode::Bisync => "bisync",
        }
    }

    /// Whether running it can delete or overwrite data (worth confirming first).
    pub fn destructive(self) -> bool {
        matches!(self, SyncMode::Mirror | SyncMode::Bisync)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffState {
    /// Identical on both sides.
    Match,
    /// Present on both, contents differ (size or hash).
    Differ,
    /// Only on the source (left) — would be created on the destination.
    SrcOnly,
    /// Only on the destination (right) — would be deleted by a mirror sync.
    DstOnly,
    /// Could not be checked (hash/read error).
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEntry {
    pub path: String,
    pub state: DiffState,
}

/// Parse an `operations/check` result (called with `match=true`) into entries
/// sorted by path. rclone reports paths split across arrays; `missingOnDst` is a
/// file present on the source but not the destination (src-only), and vice versa.
pub fn parse_check(v: &Value) -> Vec<DiffEntry> {
    let strings = |key: &str| -> Vec<String> {
        v.get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    let mut push = |key: &str, state: DiffState| {
        out.extend(strings(key).into_iter().map(|path| DiffEntry { path, state }));
    };
    push("missingOnDst", DiffState::SrcOnly);
    push("missingOnSrc", DiffState::DstOnly);
    push("differ", DiffState::Differ);
    push("error", DiffState::Error);
    push("match", DiffState::Match);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_check_arrays_to_states() {
        let v = json!({
            "differ": ["diff.txt"],
            "error": [],
            "match": ["same.txt"],
            "missingOnDst": ["onlysrc.txt"],
            "missingOnSrc": ["onlydst.txt"],
            "status": "3 differences found",
            "success": false,
        });
        let got = parse_check(&v);
        assert_eq!(
            got,
            vec![
                DiffEntry { path: "diff.txt".into(), state: DiffState::Differ },
                DiffEntry { path: "onlydst.txt".into(), state: DiffState::DstOnly },
                DiffEntry { path: "onlysrc.txt".into(), state: DiffState::SrcOnly },
                DiffEntry { path: "same.txt".into(), state: DiffState::Match },
            ]
        );
    }

    #[test]
    fn missing_arrays_yield_empty() {
        assert!(parse_check(&json!({})).is_empty());
    }
}
