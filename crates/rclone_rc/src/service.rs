use thiserror::Error;
use tokio::runtime::Handle;
use tokio::sync::oneshot;

use crate::client::{ConfigStep, Entry, JobStatus, Provider, RcClient, RemoteInfo, Stats};
use crate::RcError;

/// Aborts a spawned task when dropped — used so an unfinished interactive
/// config request is cancelled if the caller drops the future.
struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Rc(#[from] RcError),
    #[error("backend task cancelled")]
    Cancelled,
}

/// Whether a paste keeps the source (copy) or removes it (move/cut).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferMode {
    Copy,
    Move,
}

impl TransferMode {
    fn dir_method(self) -> &'static str {
        match self {
            Self::Copy => "sync/copy",
            Self::Move => "sync/move",
        }
    }

    /// Literal-path method (no globbing) for a single file.
    fn file_method(self) -> &'static str {
        match self {
            Self::Copy => "operations/copyfile",
            Self::Move => "operations/movefile",
        }
    }

    /// rclone CLI subcommand equivalent, for displaying the operation.
    pub fn cli_verb(self, is_dir: bool) -> &'static str {
        match (self, is_dir) {
            (Self::Copy, true) => "copy",
            (Self::Copy, false) => "copyto",
            (Self::Move, true) => "move",
            (Self::Move, false) => "moveto",
        }
    }
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// `(parent, name)` for a `/`-separated path; parent is empty at the root.
fn split_parent(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some((p, n)) => (p.to_string(), n.to_string()),
        None => (String::new(), path.to_string()),
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Bridge between the UI executor and the rclone daemon: RC calls run on the
/// tokio runtime, results return over a oneshot the UI awaits. Cloneable.
#[derive(Clone)]
pub struct Service {
    handle: Handle,
    client: RcClient,
}

impl Service {
    pub fn new(handle: Handle, client: RcClient) -> Self {
        Self { handle, client }
    }

    /// Run an RC call on the tokio runtime, awaiting its result over a oneshot.
    async fn run<T, Fut>(
        &self,
        call: impl FnOnce(RcClient) -> Fut + Send + 'static,
    ) -> Result<T, ServiceError>
    where
        T: Send + 'static,
        Fut: std::future::Future<Output = Result<T, RcError>> + Send,
    {
        let (tx, rx) = oneshot::channel();
        let client = self.client.clone();
        self.handle.spawn(async move {
            let _ = tx.send(call(client).await);
        });
        rx.await.map_err(|_| ServiceError::Cancelled)?.map_err(Into::into)
    }

    /// Like [`run`], but dropping the returned future aborts the request — which
    /// disconnects the client so rclone tears down any pending OAuth callback server.
    async fn run_cancellable<T, Fut>(
        &self,
        call: impl FnOnce(RcClient) -> Fut + Send + 'static,
    ) -> Result<T, ServiceError>
    where
        T: Send + 'static,
        Fut: std::future::Future<Output = Result<T, RcError>> + Send,
    {
        let (tx, rx) = oneshot::channel();
        let client = self.client.clone();
        let task = self.handle.spawn(async move {
            let _ = tx.send(call(client).await);
        });
        let _abort = AbortOnDrop(task.abort_handle());
        rx.await.map_err(|_| ServiceError::Cancelled)?.map_err(Into::into)
    }

    /// Liveness check against the rc daemon (`rc/noop`).
    pub async fn ping(&self) -> Result<(), ServiceError> {
        self.run(|c| async move { c.noop().await }).await
    }

    pub async fn list_remotes(&self) -> Result<Vec<String>, ServiceError> {
        self.run(|c| async move { c.list_remotes().await }).await
    }

    pub async fn remotes(&self) -> Result<Vec<RemoteInfo>, ServiceError> {
        self.run(|c| async move { c.remotes().await }).await
    }

    /// Delete a configured remote.
    pub async fn config_delete(&self, name: String) -> Result<(), ServiceError> {
        self.run(move |c| async move { c.config_delete(&name).await }).await
    }

    /// Stop rclone's local OAuth webserver (best-effort; errors if none running).
    pub async fn config_oauth_stop(&self) -> Result<(), ServiceError> {
        self.run(|c| async move { c.config_oauth_stop().await }).await
    }

    /// Configurable backends and their option schemas.
    pub async fn config_providers(&self) -> Result<Vec<Provider>, ServiceError> {
        self.run(|c| async move { c.config_providers().await }).await
    }

    /// Stored parameters of a remote, for editing.
    pub async fn config_get(
        &self,
        name: String,
    ) -> Result<serde_json::Map<String, serde_json::Value>, ServiceError> {
        self.run(move |c| async move { c.config_get(&name).await }).await
    }

    /// One step of interactive remote creation.
    pub async fn config_create(
        &self,
        name: String,
        kind: String,
        parameters: serde_json::Value,
        opt: serde_json::Value,
    ) -> Result<ConfigStep, ServiceError> {
        self.run_cancellable(move |c| async move { c.config_create(&name, &kind, parameters, opt).await }).await
    }

    /// One step of interactive remote editing.
    pub async fn config_update(
        &self,
        name: String,
        parameters: serde_json::Value,
        opt: serde_json::Value,
    ) -> Result<ConfigStep, ServiceError> {
        self.run_cancellable(move |c| async move { c.config_update(&name, parameters, opt).await }).await
    }

    /// Download `remote:path` into the local `dest` dir as an async job.
    pub async fn download(
        &self,
        remote: String,
        path: String,
        is_dir: bool,
        dest: std::path::PathBuf,
        group: String,
    ) -> Result<u64, ServiceError> {
        let name = basename(&path);
        let dest = dest.to_string_lossy().into_owned();
        let (method, params) = if is_dir {
            ("sync/copy", serde_json::json!({ "srcFs": format!("{remote}:{path}"), "dstFs": join(&dest, &name) }))
        } else {
            (
                "operations/copyfile",
                serde_json::json!({ "srcFs": format!("{remote}:"), "srcRemote": path, "dstFs": dest, "dstRemote": name }),
            )
        };
        self.submit(method, params, group).await
    }

    /// Copy or move `src_remote:src_path` into the `dst_remote:dst_dir` directory
    /// as an async job (cross-remote paste).
    ///
    /// Uses literal-path operations (`copyfile`/`movefile` for files, `sync` for
    /// dirs) so names with glob metacharacters (`[`, `*`, `?`, …) transfer
    /// correctly — an include-filter approach silently matches nothing for them.
    pub async fn paste(
        &self,
        src_remote: String,
        src_path: String,
        is_dir: bool,
        dst_remote: String,
        dst_dir: String,
        mode: TransferMode,
        group: String,
    ) -> Result<u64, ServiceError> {
        let name = basename(&src_path);
        let dst_path = join(&dst_dir, &name);
        let (method, params) = if is_dir {
            (
                mode.dir_method(),
                serde_json::json!({
                    "srcFs": format!("{src_remote}:{src_path}"),
                    "dstFs": format!("{dst_remote}:{dst_path}"),
                }),
            )
        } else {
            (
                mode.file_method(),
                serde_json::json!({
                    "srcFs": format!("{src_remote}:"),
                    "srcRemote": src_path,
                    "dstFs": format!("{dst_remote}:"),
                    "dstRemote": dst_path,
                }),
            )
        };
        self.submit(method, params, group).await
    }

    /// Move `remote:from` to `remote:to` within the same remote (rename) as an
    /// async job.
    pub async fn move_to(
        &self,
        remote: String,
        from: String,
        to: String,
        is_dir: bool,
        group: String,
    ) -> Result<u64, ServiceError> {
        let (method, params) = if is_dir {
            (
                "sync/move",
                serde_json::json!({ "srcFs": format!("{remote}:{from}"), "dstFs": format!("{remote}:{to}") }),
            )
        } else {
            (
                "operations/movefile",
                serde_json::json!({
                    "srcFs": format!("{remote}:"),
                    "srcRemote": from,
                    "dstFs": format!("{remote}:"),
                    "dstRemote": to,
                }),
            )
        };
        self.submit(method, params, group).await
    }

    /// Create directory `remote:path` as an async job.
    pub async fn mkdir(&self, remote: String, path: String, group: String) -> Result<u64, ServiceError> {
        self.submit("operations/mkdir", serde_json::json!({ "fs": format!("{remote}:"), "remote": path }), group)
            .await
    }

    /// Upload a local file or directory `local` into `remote:dst_dir` as an
    /// async job.
    pub async fn upload(
        &self,
        local: String,
        remote: String,
        dst_dir: String,
        is_dir: bool,
        group: String,
    ) -> Result<u64, ServiceError> {
        let (parent, name) = split_parent(&local);
        let (method, params) = if is_dir {
            (
                TransferMode::Copy.dir_method(),
                serde_json::json!({
                    "srcFs": local,
                    "dstFs": format!("{remote}:{}", join(&dst_dir, &name)),
                }),
            )
        } else {
            (
                TransferMode::Copy.file_method(),
                serde_json::json!({
                    "srcFs": parent,
                    "srcRemote": name,
                    "dstFs": format!("{remote}:"),
                    "dstRemote": join(&dst_dir, &name),
                }),
            )
        };
        self.submit(method, params, group).await
    }

    /// Read up to `max_bytes` of `remote:path`'s content (for previews).
    pub async fn read_file(
        &self,
        remote: String,
        path: String,
        max_bytes: u64,
    ) -> Result<Vec<u8>, ServiceError> {
        self.run(move |c| async move { c.fetch_object(&remote, &path, max_bytes).await }).await
    }

    /// Submit an async job in stats group `group`, returning the job id.
    async fn submit(
        &self,
        method: &'static str,
        params: serde_json::Value,
        group: String,
    ) -> Result<u64, ServiceError> {
        tracing::debug!(method, ?params, "submit job");
        self.run(move |c| async move { c.call_async(method, params, &group).await }).await
    }

    /// Permanently delete `remote:path` as an async job. `is_dir` (from rclone's
    /// own listing) picks `purge` for a directory vs `deletefile` for a file.
    pub async fn delete(
        &self,
        remote: String,
        path: String,
        is_dir: bool,
        group: String,
    ) -> Result<u64, ServiceError> {
        let method = if is_dir { "operations/purge" } else { "operations/deletefile" };
        self.submit(method, serde_json::json!({ "fs": format!("{remote}:"), "remote": path }), group).await
    }

    pub async fn job_status(&self, jobid: u64) -> Result<JobStatus, ServiceError> {
        self.run(move |c| async move { c.job_status(jobid).await }).await
    }

    pub async fn stats(&self, group: String) -> Result<Stats, ServiceError> {
        self.run(move |c| async move { c.stats(&group).await }).await
    }

    pub async fn job_stop(&self, jobid: u64) -> Result<(), ServiceError> {
        self.run(move |c| async move { c.job_stop(jobid).await }).await
    }

    pub async fn list_dir(&self, remote: &str, path: &str) -> Result<Vec<Entry>, ServiceError> {
        // "start" with no matching "done"/"failed" marks a listing still in flight.
        tracing::debug!(remote, path, "list dir start");
        let start = std::time::Instant::now();
        let (fs, owned_path) = (format!("{remote}:"), path.to_string());
        let result = self.run(move |c| async move { c.list(&fs, &owned_path).await }).await;
        let ms = start.elapsed().as_millis() as u64;
        match &result {
            Ok(entries) => tracing::debug!(remote, path, count = entries.len(), elapsed_ms = ms, "list dir done"),
            Err(e) => tracing::warn!(remote, path, elapsed_ms = ms, error = %e, "list dir failed"),
        }
        result
    }
}
