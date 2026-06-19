use std::sync::{Arc, RwLock};

use thiserror::Error;
use tokio::runtime::Handle;
use tokio::sync::{oneshot, Mutex};

use std::path::PathBuf;

use crate::client::{
    ConfigPaths, ConfigStep, Entry, JobStatus, Provider, RcClient, RemoteInfo, Stats,
};
use crate::daemon::{Daemon, SharedDaemon};
use crate::mount::{MountConfig, Mounts, SharedMounts};
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
    #[error("daemon restart: {0}")]
    Daemon(String),
    #[error("mount: {0}")]
    Mount(String),
    #[error("invalid arguments for {0}")]
    InvalidArgs(&'static str),
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

    fn operation(self) -> Operation {
        match self {
            Self::Copy => Operation::Copy,
            Self::Move => Operation::Move,
        }
    }

    /// rclone CLI subcommand equivalent, for displaying the operation.
    pub fn cli_verb(self, is_dir: bool) -> &'static str {
        self.operation().cli_verb(is_dir)
    }
}

use crate::ops::{basename, join, only_file, split_parent, ArgValue, Operation};

/// Bridge between the UI executor and the rclone daemon: RC calls run on the
/// tokio runtime, results return over a oneshot the UI awaits. Cloneable.
///
/// Owns the daemon so it can be restarted: the client lives behind a swap-able
/// handle, so when [`restart_daemon`] spawns a fresh `rcd` on a new port every
/// clone of the service picks up the new endpoint automatically.
///
/// [`restart_daemon`]: Service::restart_daemon
#[derive(Clone)]
pub struct Service {
    handle: Handle,
    client: Arc<RwLock<RcClient>>,
    daemon: SharedDaemon,
    rclone: PathBuf,
    mounts: SharedMounts,
}

impl Service {
    /// Take ownership of a started [`Daemon`], exposing it as a restartable
    /// service. `rclone` (the binary) backs the no-install NFS mounts.
    pub fn from_daemon(handle: Handle, daemon: Daemon, rclone: PathBuf) -> Self {
        let client = Arc::new(RwLock::new(daemon.client().clone()));
        Self {
            handle,
            client,
            daemon: Arc::new(Mutex::new(daemon)),
            rclone,
            mounts: Arc::new(Mutex::new(Mounts::default())),
        }
    }

    /// Install termination-signal cleanup for the daemon and mounts.
    pub fn install_signal_cleanup(&self) {
        #[cfg(any(unix, windows))]
        crate::daemon::install_signal_cleanup(
            &self.handle,
            self.daemon.clone(),
            self.mounts.clone(),
        );
    }

    /// Snapshot the current RC client (cheap clone; changes after a restart).
    fn client(&self) -> RcClient {
        self.client.read().unwrap().clone()
    }

    /// Restart the daemon: stop `rcd`, spawn a fresh one, and swap in the new
    /// client so all service clones use the new endpoint.
    pub async fn restart_daemon(&self) -> Result<(), ServiceError> {
        let (daemon, client) = (self.daemon.clone(), self.client.clone());
        let (tx, rx) = oneshot::channel();
        self.handle.spawn(async move {
            let result = daemon.lock().await.restart().await;
            let sent = match result {
                Ok(new_client) => {
                    *client.write().unwrap() = new_client;
                    Ok(())
                }
                Err(e) => Err(ServiceError::Daemon(e.to_string())),
            };
            let _ = tx.send(sent);
        });
        rx.await.map_err(|_| ServiceError::Cancelled)?
    }

    /// Gracefully stop the daemon and tear down every mount (on app exit).
    pub async fn shutdown(&self) {
        self.mounts.lock().await.unmount_all().await;
        self.daemon.lock().await.shutdown().await;
    }

    /// Mount `remote:` at `mountpoint` with `config`'s VFS/mount flags (no macFUSE, no sudo).
    pub async fn mount_remote(
        &self,
        remote: String,
        mountpoint: PathBuf,
        config: MountConfig,
    ) -> Result<(), ServiceError> {
        let (mounts, rclone) = (self.mounts.clone(), self.rclone.clone());
        let (tx, rx) = oneshot::channel();
        self.handle.spawn(async move {
            let r = mounts
                .lock()
                .await
                .mount(&rclone, &remote, &mountpoint, &config)
                .await
                .map_err(|e| ServiceError::Mount(e.to_string()));
            let _ = tx.send(r);
        });
        rx.await.map_err(|_| ServiceError::Cancelled)?
    }

    /// rclone's resolved config/cache/temp paths (`config/paths`), as detected
    /// per-OS by rclone itself.
    pub async fn config_paths(&self) -> Result<ConfigPaths, ServiceError> {
        self.run(|c| async move { c.config_paths().await }).await
    }

    /// Unmount `remote`.
    pub async fn unmount_remote(&self, remote: String) -> Result<(), ServiceError> {
        let mounts = self.mounts.clone();
        let (tx, rx) = oneshot::channel();
        self.handle.spawn(async move {
            let r = mounts
                .lock()
                .await
                .unmount(&remote)
                .await
                .map_err(|e| ServiceError::Mount(e.to_string()));
            let _ = tx.send(r);
        });
        rx.await.map_err(|_| ServiceError::Cancelled)?
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
        let client = self.client();
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
        let client = self.client();
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
            // Single file: listing-based copy restricted to exactly this file, so it
            // works on backends whose NewObject can't resolve a path (e.g. torbox).
            let (parent, leaf) = split_parent(&path);
            (
                "sync/copy",
                serde_json::json!({ "srcFs": format!("{remote}:{parent}"), "dstFs": dest, "_filter": { "IncludeRule": [only_file(&leaf)] } }),
            )
        };
        self.submit(method, params, group).await
    }

    /// Build and submit a registry [`Operation`] from resolved `args` — the
    /// single dispatch + validation point shared by the context menu and palette.
    pub async fn run_operation(
        &self,
        op: Operation,
        args: Vec<ArgValue>,
        group: String,
    ) -> Result<u64, ServiceError> {
        let (method, params) = op.build(&args).ok_or(ServiceError::InvalidArgs(op.label()))?;
        self.submit(method, params, group).await
    }

    /// Copy/move `src_remote:src_path` into the `dst_remote:dst_dir` directory.
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
        let args = vec![
            ArgValue::Path { remote: src_remote, path: src_path, is_dir },
            ArgValue::Path { remote: dst_remote, path: dst_dir, is_dir: true },
        ];
        self.run_operation(mode.operation(), args, group).await
    }

    /// Rename `remote:from` to `new_name` within its current directory, as an
    /// async job.
    pub async fn move_to(
        &self,
        remote: String,
        from: String,
        new_name: String,
        is_dir: bool,
        group: String,
    ) -> Result<u64, ServiceError> {
        let args = vec![ArgValue::Path { remote, path: from, is_dir }, ArgValue::Name(new_name)];
        self.run_operation(Operation::Rename, args, group).await
    }

    /// Create directory `remote:path` as an async job.
    pub async fn mkdir(&self, remote: String, path: String, group: String) -> Result<u64, ServiceError> {
        let (parent, name) = split_parent(&path);
        let args = vec![ArgValue::Path { remote, path: parent, is_dir: true }, ArgValue::Name(name)];
        self.run_operation(Operation::MakeDir, args, group).await
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
                    "srcRemote": &name,
                    "dstFs": format!("{remote}:{dst_dir}"),
                    "dstRemote": &name,
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

    /// Call a read-only RC method and return its raw JSON (for info ops).
    pub async fn query(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ServiceError> {
        self.run(move |c| async move { c.call::<serde_json::Value>(method, &params).await }).await
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
        let args = vec![ArgValue::Path { remote, path, is_dir }];
        self.run_operation(Operation::Delete, args, group).await
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

    /// Recursive word search (all query words, AND). rclone bounds the files
    /// server-side but returns every directory, so the results are narrowed here
    /// with the same [`Matcher`] used for the in-folder filter.
    pub async fn search(&self, remote: &str, path: &str, query: &str) -> Result<Vec<Entry>, ServiceError> {
        tracing::debug!(remote, path, query, "search start");
        let start = std::time::Instant::now();
        let (fs, owned_path, owned_query) = (format!("{remote}:"), path.to_string(), query.to_string());
        let mut result = self
            .run(move |c| async move { c.list_filtered(&fs, &owned_path, &owned_query).await })
            .await;
        if let Ok(entries) = &mut result {
            let matcher = crate::client::Matcher::new(query);
            entries.retain(|e| matcher.matches(&e.name));
        }
        let ms = start.elapsed().as_millis() as u64;
        match &result {
            Ok(entries) => tracing::debug!(remote, path, count = entries.len(), elapsed_ms = ms, "search done"),
            Err(e) => tracing::warn!(remote, path, elapsed_ms = ms, error = %e, "search failed"),
        }
        result
    }
}
