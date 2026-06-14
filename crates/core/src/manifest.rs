use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// An artifact created OUTSIDE the app root that uninstall must remove.
///
/// The OS owns this storage (File Provider domains, CFAPI sync roots, FUSE
/// mounts, registry keys), so teardown goes through the owning API, never `rm`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Artifact {
    FileProviderDomain { identifier: String },
    CloudFilesSyncRoot { path: PathBuf },
    FuseMount { path: PathBuf },
    RegistryKey { key: String },
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("manifest i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("manifest record parse: {0}")]
    Parse(#[from] serde_json::Error),
}

/// Replay outcome: a manifest read failure, or per-artifact teardown failures.
#[derive(Debug)]
pub enum TeardownErrors<E> {
    Manifest(ManifestError),
    Failures(Vec<(Artifact, E)>),
}

/// Append-only log of external artifacts at a fixed path.
#[derive(Debug, Clone)]
pub struct Manifest {
    path: PathBuf,
}

impl Manifest {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Record a newly created external artifact, one JSON record per line.
    pub fn record(&self, artifact: &Artifact) -> Result<(), ManifestError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        writeln!(file, "{}", serde_json::to_string(artifact)?)?;
        Ok(())
    }

    /// All recorded artifacts, de-duplicated, oldest first.
    pub fn artifacts(&self) -> Result<Vec<Artifact>, ManifestError> {
        let file = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let artifact: Artifact = serde_json::from_str(&line)?;
            if !out.contains(&artifact) {
                out.push(artifact);
            }
        }
        Ok(out)
    }

    /// Replay teardown for every recorded artifact via `handler`.
    ///
    /// On full success the manifest file is removed. If any artifact fails, the
    /// file is left intact so a later run can retry the remainder.
    pub fn replay<F, E>(&self, mut handler: F) -> Result<(), TeardownErrors<E>>
    where
        F: FnMut(&Artifact) -> Result<(), E>,
    {
        let artifacts = self.artifacts().map_err(TeardownErrors::Manifest)?;
        let mut failures = Vec::new();
        for artifact in &artifacts {
            if let Err(e) = handler(artifact) {
                failures.push((artifact.clone(), e));
            }
        }
        if failures.is_empty() {
            let _ = std::fs::remove_file(&self.path);
            Ok(())
        } else {
            Err(TeardownErrors::Failures(failures))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn record_and_dedupe() {
        let dir = tempdir().unwrap();
        let m = Manifest::new(dir.path().join("teardown.jsonl"));
        let a = Artifact::FileProviderDomain { identifier: "d1".into() };
        m.record(&a).unwrap();
        m.record(&a).unwrap();
        assert_eq!(m.artifacts().unwrap(), vec![a]);
    }

    #[test]
    fn missing_manifest_is_empty() {
        let dir = tempdir().unwrap();
        let m = Manifest::new(dir.path().join("none.jsonl"));
        assert!(m.artifacts().unwrap().is_empty());
    }

    #[test]
    fn replay_removes_file_on_success() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("teardown.jsonl");
        let m = Manifest::new(&path);
        m.record(&Artifact::FuseMount { path: "/mnt/x".into() }).unwrap();
        m.replay(|_| Ok::<(), ()>(())).unwrap();
        assert!(!path.exists());
    }
}
