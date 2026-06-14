use thiserror::Error;
use tokio::runtime::Handle;
use tokio::sync::oneshot;

use crate::client::{Entry, JobStatus, RcClient, RemoteInfo, Stats};
use crate::RcError;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Rc(#[from] RcError),
    #[error("backend task cancelled")]
    Cancelled,
}

/// Bridge between the UI executor and the rclone daemon.
///
/// RC calls run on the tokio runtime (where reqwest's reactor lives); the result
/// returns over a oneshot the UI awaits on gpui's executor. Cloneable so views
/// can hold their own handle.
#[derive(Clone)]
pub struct Service {
    handle: Handle,
    client: RcClient,
}

impl Service {
    pub fn new(handle: Handle, client: RcClient) -> Self {
        Self { handle, client }
    }

    /// Liveness check against the rc daemon (`rc/noop`).
    pub async fn ping(&self) -> Result<(), ServiceError> {
        let (tx, rx) = oneshot::channel();
        let client = self.client.clone();
        self.handle.spawn(async move {
            let _ = tx.send(client.noop().await);
        });
        rx.await.map_err(|_| ServiceError::Cancelled)?.map_err(Into::into)
    }

    pub async fn list_remotes(&self) -> Result<Vec<String>, ServiceError> {
        let (tx, rx) = oneshot::channel();
        let client = self.client.clone();
        self.handle.spawn(async move {
            let _ = tx.send(client.list_remotes().await);
        });
        rx.await.map_err(|_| ServiceError::Cancelled)?.map_err(Into::into)
    }

    pub async fn remotes(&self) -> Result<Vec<RemoteInfo>, ServiceError> {
        let (tx, rx) = oneshot::channel();
        let client = self.client.clone();
        self.handle.spawn(async move {
            let _ = tx.send(client.remotes().await);
        });
        rx.await.map_err(|_| ServiceError::Cancelled)?.map_err(Into::into)
    }

    /// Submit a download of `remote:path` into `dest` as an async rclone job in
    /// stats group `group`, returning the job id.
    ///
    /// We do not decide file-vs-directory: a single `sync/copy` from the item's
    /// parent, filtered to just that item, lets rclone resolve the type and
    /// preserves the item's name under `dest`.
    pub async fn download(
        &self,
        remote: String,
        path: String,
        dest: std::path::PathBuf,
        group: String,
    ) -> Result<u64, ServiceError> {
        let (parent, name) = match path.rsplit_once('/') {
            Some((p, n)) => (p.to_string(), n.to_string()),
            None => (String::new(), path.clone()),
        };
        let (tx, rx) = oneshot::channel();
        let client = self.client.clone();
        self.handle.spawn(async move {
            let params = serde_json::json!({
                "srcFs": format!("{remote}:{parent}"),
                "dstFs": dest.to_string_lossy(),
                "_filter": { "IncludeRule": [format!("/{name}"), format!("/{name}/**")] },
            });
            let _ = tx.send(client.call_async("sync/copy", params, &group).await);
        });
        rx.await.map_err(|_| ServiceError::Cancelled)?.map_err(Into::into)
    }

    pub async fn job_status(&self, jobid: u64) -> Result<JobStatus, ServiceError> {
        let (tx, rx) = oneshot::channel();
        let client = self.client.clone();
        self.handle.spawn(async move {
            let _ = tx.send(client.job_status(jobid).await);
        });
        rx.await.map_err(|_| ServiceError::Cancelled)?.map_err(Into::into)
    }

    pub async fn stats(&self, group: String) -> Result<Stats, ServiceError> {
        let (tx, rx) = oneshot::channel();
        let client = self.client.clone();
        self.handle.spawn(async move {
            let _ = tx.send(client.stats(&group).await);
        });
        rx.await.map_err(|_| ServiceError::Cancelled)?.map_err(Into::into)
    }

    pub async fn job_stop(&self, jobid: u64) -> Result<(), ServiceError> {
        let (tx, rx) = oneshot::channel();
        let client = self.client.clone();
        self.handle.spawn(async move {
            let _ = tx.send(client.job_stop(jobid).await);
        });
        rx.await.map_err(|_| ServiceError::Cancelled)?.map_err(Into::into)
    }

    pub async fn list_dir(&self, remote: &str, path: &str) -> Result<Vec<Entry>, ServiceError> {
        let (tx, rx) = oneshot::channel();
        let client = self.client.clone();
        let fs = format!("{remote}:");
        let path = path.to_string();
        self.handle.spawn(async move {
            let _ = tx.send(client.list(&fs, &path).await);
        });
        rx.await.map_err(|_| ServiceError::Cancelled)?.map_err(Into::into)
    }
}
