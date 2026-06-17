//! Single source of truth for file operations.
//!
//! Each [`Operation`] declares its label, its ordered argument requirements
//! ([`ArgSpec`]), whether it belongs in the right-click context menu, and how it
//! maps to an rclone rc `(method, params)`. The context menu, command palette,
//! executor, and validation all derive from here — no per-operation param
//! building duplicated elsewhere.

use serde_json::{json, Value};

/// A file operation the app can perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Copy,
    Move,
    Sync,
    Delete,
    Cleanup,
    Rmdir,
    Rmdirs,
    MakeDir,
    Rename,
    CopyUrl,
    SetTier,
}

/// What an argument resolves to — drives the palette's input mode for the stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// An existing file or directory: remote + path (+ `is_dir`). Path-completed.
    SourcePath,
    /// A destination directory: remote + path. Path-completed (dirs only).
    DestDir,
    /// A whole remote (no folder descent), e.g. for remote-level queries.
    Remote,
    /// Free text (no completion), e.g. a new folder or rename name.
    Name,
}

/// One required argument, in prompt order.
#[derive(Debug, Clone, Copy)]
pub struct ArgSpec {
    pub kind: ArgKind,
    pub label: &'static str,
}

const fn arg(kind: ArgKind, label: &'static str) -> ArgSpec {
    ArgSpec { kind, label }
}

const TRANSFER_ARGS: &[ArgSpec] = &[arg(ArgKind::SourcePath, "Source"), arg(ArgKind::DestDir, "Destination")];
const DELETE_ARGS: &[ArgSpec] = &[arg(ArgKind::SourcePath, "Target")];
const MKDIR_ARGS: &[ArgSpec] = &[arg(ArgKind::DestDir, "In"), arg(ArgKind::Name, "Folder name")];
const RENAME_ARGS: &[ArgSpec] = &[arg(ArgKind::SourcePath, "Target"), arg(ArgKind::Name, "New name")];
const COPYURL_ARGS: &[ArgSpec] = &[arg(ArgKind::Name, "URL"), arg(ArgKind::DestDir, "Destination")];
const SETTIER_ARGS: &[ArgSpec] = &[arg(ArgKind::SourcePath, "Target"), arg(ArgKind::Name, "Tier")];

/// A resolved argument value supplied by the caller, matching an [`ArgSpec`].
#[derive(Debug, Clone)]
pub enum ArgValue {
    Path { remote: String, path: String, is_dir: bool },
    Name(String),
}

impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Operation::Copy => "Copy",
            Operation::Move => "Move",
            Operation::Sync => "Sync",
            Operation::Delete => "Delete",
            Operation::Cleanup => "Clean Up",
            Operation::Rmdir => "Remove Empty Folder",
            Operation::Rmdirs => "Remove Empty Folders",
            Operation::MakeDir => "New Folder",
            Operation::Rename => "Rename",
            Operation::CopyUrl => "Copy URL",
            Operation::SetTier => "Set Tier",
        }
    }

    /// Required arguments, in the order the UI should collect them.
    pub fn args(self) -> &'static [ArgSpec] {
        match self {
            Operation::Copy | Operation::Move | Operation::Sync => TRANSFER_ARGS,
            Operation::Delete | Operation::Cleanup | Operation::Rmdir | Operation::Rmdirs => {
                DELETE_ARGS
            }
            Operation::MakeDir => MKDIR_ARGS,
            Operation::Rename => RENAME_ARGS,
            Operation::CopyUrl => COPYURL_ARGS,
            Operation::SetTier => SETTIER_ARGS,
        }
    }

    /// Removes or destroys data — the UI should confirm before running.
    pub fn destructive(self) -> bool {
        matches!(self, Operation::Delete | Operation::Cleanup)
    }

    /// rclone CLI subcommand for display in the transfer queue.
    pub fn cli_verb(self, is_dir: bool) -> &'static str {
        match (self, is_dir) {
            (Operation::Copy, true) => "copy",
            (Operation::Copy, false) => "copyto",
            (Operation::Move, true) => "move",
            (Operation::Move, false) => "moveto",
            (Operation::Sync, _) => "sync",
            (Operation::Delete, true) => "purge",
            (Operation::Delete, false) => "delete",
            (Operation::Cleanup, _) => "cleanup",
            (Operation::Rmdir, _) => "rmdir",
            (Operation::Rmdirs, _) => "rmdirs",
            (Operation::MakeDir, _) => "mkdir",
            (Operation::Rename, true) => "move",
            (Operation::Rename, false) => "moveto",
            (Operation::CopyUrl, _) => "copyurl",
            (Operation::SetTier, _) => "settier",
        }
    }

    /// Build the rclone rc `(method, params)` for `args`, or `None` if `args`
    /// don't satisfy [`Operation::args`] (the single validation point).
    pub fn build(self, args: &[ArgValue]) -> Option<(&'static str, Value)> {
        match self {
            Operation::Copy | Operation::Move => {
                let [src, dst] = args else { return None };
                let (sr, sp, is_dir) = src.as_path()?;
                let (dr, dd, _) = dst.as_path()?;
                let mv = matches!(self, Operation::Move);
                Some(if is_dir {
                    // Paste semantics: drop the dir into the dest under its own name.
                    let dst_path = join(dd, &basename(sp));
                    let method = if mv { "sync/move" } else { "sync/copy" };
                    (method, json!({ "srcFs": format!("{sr}:{sp}"), "dstFs": format!("{dr}:{dst_path}") }))
                } else {
                    // Single file: restrict the sync engine to exactly this file via
                    // `only_file` so it resolves by listing the parent — works on backends
                    // whose NewObject can't resolve a path (torbox), where copyfile fails.
                    let (parent, leaf) = split_parent(sp);
                    let method = if mv { "sync/move" } else { "sync/copy" };
                    (method, json!({
                        "srcFs": format!("{sr}:{parent}"),
                        "dstFs": format!("{dr}:{dd}"),
                        "_filter": { "IncludeRule": [only_file(&leaf)] },
                    }))
                })
            }
            // Make the destination dir mirror the source (one-way, deletes extras).
            Operation::Sync => {
                let [src, dst] = args else { return None };
                let (sr, sp, _) = src.as_path()?;
                let (dr, dd, _) = dst.as_path()?;
                Some(("sync/sync", json!({ "srcFs": format!("{sr}:{sp}"), "dstFs": format!("{dr}:{dd}") })))
            }
            Operation::Delete => {
                let [target] = args else { return None };
                let (r, p, is_dir) = target.as_path()?;
                Some(if is_dir {
                    ("operations/purge", json!({ "fs": format!("{r}:"), "remote": p }))
                } else {
                    // Listing-based delete restricted to exactly this file (same reason
                    // as copy: works where NewObject/deletefile can't resolve a path).
                    let (parent, leaf) = split_parent(p);
                    ("operations/delete", json!({ "fs": format!("{r}:{parent}"), "_filter": { "IncludeRule": [only_file(&leaf)] } }))
                })
            }
            // Free space / clear old versions on the whole fs at the target path.
            Operation::Cleanup => {
                let [target] = args else { return None };
                let (r, p, _) = target.as_path()?;
                Some(("operations/cleanup", json!({ "fs": format!("{r}:{p}") })))
            }
            Operation::Rmdir => {
                let [target] = args else { return None };
                let (r, p, _) = target.as_path()?;
                Some(("operations/rmdir", json!({ "fs": format!("{r}:"), "remote": p })))
            }
            Operation::Rmdirs => {
                let [target] = args else { return None };
                let (r, p, _) = target.as_path()?;
                Some(("operations/rmdirs", json!({ "fs": format!("{r}:"), "remote": p })))
            }
            Operation::MakeDir => {
                let [dir, name] = args else { return None };
                let (r, p, _) = dir.as_path()?;
                Some(("operations/mkdir", json!({ "fs": format!("{r}:"), "remote": join(p, name.as_name()?) })))
            }
            Operation::Rename => {
                let [target, name] = args else { return None };
                let (r, p, is_dir) = target.as_path()?;
                let dst = join(&split_parent(p).0, name.as_name()?);
                Some(if is_dir {
                    ("sync/move", json!({ "srcFs": format!("{r}:{p}"), "dstFs": format!("{r}:{dst}") }))
                } else {
                    let (src_fs, src_leaf) = fs_leaf(r, p);
                    let (dst_fs, dst_leaf) = fs_leaf(r, &dst);
                    ("operations/movefile", json!({ "srcFs": src_fs, "srcRemote": src_leaf, "dstFs": dst_fs, "dstRemote": dst_leaf }))
                })
            }
            // Download a URL into the destination directory (name from the URL).
            Operation::CopyUrl => {
                let [url, dst] = args else { return None };
                let url = url.as_name()?;
                let (r, d, _) = dst.as_path()?;
                Some((
                    "operations/copyurl",
                    json!({ "url": url, "fs": format!("{r}:"), "remote": d, "autoFilename": true }),
                ))
            }
            // Set the storage tier (provider-specific, e.g. S3/Azure) on the target.
            Operation::SetTier => {
                let [target, tier] = args else { return None };
                let (r, p, _) = target.as_path()?;
                Some(("operations/settier", json!({ "fs": format!("{r}:{p}"), "tier": tier.as_name()? })))
            }
        }
    }
}

impl ArgValue {
    fn as_path(&self) -> Option<(&str, &str, bool)> {
        match self {
            ArgValue::Path { remote, path, is_dir } => Some((remote, path, *is_dir)),
            ArgValue::Name(_) => None,
        }
    }

    fn as_name(&self) -> Option<&str> {
        match self {
            ArgValue::Name(s) => Some(s),
            ArgValue::Path { .. } => None,
        }
    }
}

/// Read-only query operations: they return data to display rather than running
/// a job. Single source of truth, like [`Operation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoOp {
    Size,
    About,
    Stat,
    PublicLink,
}

const SIZE_ARGS: &[ArgSpec] = &[arg(ArgKind::SourcePath, "Target")];
const ABOUT_ARGS: &[ArgSpec] = &[arg(ArgKind::Remote, "Remote")];

/// Parsed result of an [`InfoOp`]; the UI renders it (humanizing bytes etc.).
#[derive(Debug, Clone)]
pub enum InfoResult {
    Size { count: i64, bytes: i64 },
    Quota { used: Option<i64>, total: Option<i64>, free: Option<i64> },
    Stat { name: String, bytes: i64, is_dir: bool },
    Link(String),
}

impl InfoOp {
    pub fn label(self) -> &'static str {
        match self {
            InfoOp::Size => "Size",
            InfoOp::About => "About",
            InfoOp::Stat => "Stat",
            InfoOp::PublicLink => "Public Link",
        }
    }

    pub fn args(self) -> &'static [ArgSpec] {
        match self {
            InfoOp::About => ABOUT_ARGS,
            _ => SIZE_ARGS,
        }
    }

    pub fn build(self, args: &[ArgValue]) -> Option<(&'static str, Value)> {
        let [target] = args else { return None };
        let (r, p, _) = target.as_path()?;
        Some(match self {
            InfoOp::Size => ("operations/size", json!({ "fs": format!("{r}:{p}") })),
            InfoOp::About => ("operations/about", json!({ "fs": format!("{r}:") })),
            InfoOp::Stat => ("operations/stat", json!({ "fs": format!("{r}:"), "remote": p })),
            InfoOp::PublicLink => ("operations/publiclink", json!({ "fs": format!("{r}:"), "remote": p })),
        })
    }

    /// Read the relevant fields out of rclone's JSON response.
    pub fn parse(self, v: &Value) -> Option<InfoResult> {
        Some(match self {
            InfoOp::Size => InfoResult::Size {
                count: v.get("count").and_then(Value::as_i64).unwrap_or(0),
                bytes: v.get("bytes").and_then(Value::as_i64).unwrap_or(0),
            },
            InfoOp::About => InfoResult::Quota {
                used: v.get("used").and_then(Value::as_i64),
                total: v.get("total").and_then(Value::as_i64),
                free: v.get("free").and_then(Value::as_i64),
            },
            InfoOp::Stat => {
                let item = v.get("item").filter(|i| !i.is_null())?;
                InfoResult::Stat {
                    name: item.get("Name").and_then(Value::as_str).unwrap_or_default().to_string(),
                    bytes: item.get("Size").and_then(Value::as_i64).unwrap_or(0),
                    is_dir: item.get("IsDir").and_then(Value::as_bool).unwrap_or(false),
                }
            }
            InfoOp::PublicLink => InfoResult::Link(v.get("url")?.as_str()?.to_string()),
        })
    }
}

pub(crate) fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// `(parent, name)` for a `/`-separated path; parent is empty at the root.
pub fn split_parent(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some((p, n)) => (p.to_string(), n.to_string()),
        None => (String::new(), path.to_string()),
    }
}

/// Join a `/`-separated directory and name (no extra slash at the root).
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Address a single file object the way rclone's own `NewFsFile` does: the fs is
/// the file's parent directory and the remote is the leaf name. Pure split.
pub(crate) fn fs_leaf(remote: &str, path: &str) -> (String, String) {
    let (parent, leaf) = split_parent(path);
    (format!("{remote}:{parent}"), leaf)
}

/// An `IncludeRule` matching exactly `leaf` at the fs root (anchored, glob chars
/// escaped) — lets the sync engine resolve a single file by listing, for backends
/// whose `NewObject` can't resolve a path (e.g. torbox). See the `glob_escape` test.
pub(crate) fn only_file(leaf: &str) -> String {
    let mut out = String::with_capacity(leaf.len() + 1);
    out.push('/');
    for c in leaf.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_escape() {
        assert_eq!(only_file("movie.mp4"), "/movie.mp4");
        assert_eq!(only_file("a[1].txt"), "/a\\[1\\].txt");
        assert_eq!(only_file("b*?{x}.mkv"), "/b\\*\\?\\{x\\}.mkv");
        assert_eq!(only_file("back\\slash"), "/back\\\\slash");
    }

    #[test]
    fn fs_leaf_splits_parent() {
        assert_eq!(fs_leaf("r", "dir/file.txt"), ("r:dir".into(), "file.txt".into()));
        assert_eq!(fs_leaf("r", "file.txt"), ("r:".into(), "file.txt".into()));
    }
}
