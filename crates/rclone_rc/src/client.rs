use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;

/// State of an async rclone job (`job/status`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct JobStatus {
    #[serde(default)]
    pub finished: bool,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub error: String,
    /// Run time in seconds; set by rclone once finished.
    #[serde(default)]
    pub duration: f64,
}

/// Transfer progress for a job's stats group (`core/stats`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Stats {
    #[serde(default)]
    pub bytes: u64,
    #[serde(default, rename = "totalBytes")]
    pub total_bytes: u64,
    #[serde(default)]
    pub speed: f64,
    #[serde(default)]
    pub eta: Option<u64>,
    /// Files transferred so far in this group.
    #[serde(default)]
    pub transfers: u64,
    /// Total files queued for this group (0 until rclone has scanned).
    #[serde(default, rename = "totalTransfers")]
    pub total_transfers: u64,
    /// Live elapsed seconds since the stats group started.
    #[serde(default, rename = "elapsedTime")]
    pub elapsed_time: f64,
}

/// A configured remote and its backend type (e.g. `drive`, `s3`).
#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    pub kind: String,
}

/// A configurable rclone backend (`config/providers`).
#[derive(Debug, Clone, Deserialize)]
pub struct Provider {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Description", default)]
    pub description: String,
    #[serde(rename = "Options", default)]
    pub options: Vec<RemoteOption>,
}

/// One backend option, also the shape of an interactive config question
/// (`ConfigOut.Option`). Field names match rclone's JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteOption {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Help", default)]
    pub help: String,
    #[serde(rename = "Type", default)]
    pub kind: String,
    #[serde(rename = "DefaultStr", default)]
    pub default: String,
    #[serde(rename = "Required", default)]
    pub required: bool,
    #[serde(rename = "IsPassword", default)]
    pub is_password: bool,
    #[serde(rename = "Advanced", default)]
    pub advanced: bool,
    #[serde(rename = "Examples", default)]
    pub examples: Vec<OptionExample>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OptionExample {
    #[serde(rename = "Value", default)]
    pub value: String,
    #[serde(rename = "Help", default)]
    pub help: String,
}

/// One step of the interactive config flow (`config/create`/`config/update`).
/// A non-empty `state` means `option` is the next question to ask; an empty
/// `state` with no error means the remote is fully configured.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConfigStep {
    #[serde(rename = "State", default)]
    pub state: String,
    #[serde(rename = "Option", default)]
    pub option: Option<RemoteOption>,
    #[serde(rename = "Error", default)]
    pub error: String,
}

/// rclone's resolved on-disk paths (`config/paths`) — detected per-OS by rclone.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConfigPaths {
    pub config: String,
    pub cache: String,
    pub temp: String,
}

/// One entry from `operations/list`. Field names match rclone's JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    #[serde(rename = "Name")]
    pub name: String,
    /// Path relative to the remote root; used to descend into directories.
    #[serde(rename = "Path")]
    pub path: String,
    #[serde(rename = "Size")]
    pub size: i64,
    /// RFC3339; empty when the backend omits it. Sorts chronologically as text.
    #[serde(rename = "ModTime", default)]
    pub mod_time: String,
    #[serde(rename = "IsDir")]
    pub is_dir: bool,
}

#[derive(Debug, Error)]
pub enum RcError {
    #[error("rc request: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{message}")]
    Status { status: u16, message: String },
}

/// Pull rclone's human-readable `error` field out of an RC error body, falling
/// back to the trimmed raw body when it isn't the expected JSON shape.
fn rc_error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| body.trim().to_string())
}

/// Authenticated client for the rclone remote-control API.
#[derive(Debug, Clone)]
pub struct RcClient {
    base_url: String,
    user: String,
    pass: String,
    http: Client,
}

impl RcClient {
    pub fn new(base_url: String, user: String, pass: String) -> Self {
        Self { base_url, user, pass, http: Client::new() }
    }

    /// POST to an RC method with a JSON body, returning the decoded response.
    /// Polls log at `trace`, other calls at `debug`, failures at `warn`.
    pub async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        body: &Value,
    ) -> Result<T, RcError> {
        let start = std::time::Instant::now();
        let result = self.call_inner(method, body).await;
        let ms = start.elapsed().as_millis() as u64;
        match &result {
            Err(e) => tracing::warn!(method, elapsed_ms = ms, error = %e, "rc call failed"),
            Ok(_) if matches!(method, "job/status" | "core/stats") => {
                tracing::trace!(method, elapsed_ms = ms, "rc call")
            }
            Ok(_) => tracing::debug!(method, elapsed_ms = ms, "rc call"),
        }
        result
    }

    async fn call_inner<T: DeserializeOwned>(
        &self,
        method: &str,
        body: &Value,
    ) -> Result<T, RcError> {
        let resp = self
            .http
            .post(format!("{}/{method}", self.base_url))
            .basic_auth(&self.user, Some(&self.pass))
            .json(body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(RcError::Status {
                status: status.as_u16(),
                message: rc_error_message(&body),
            });
        }
        Ok(resp.json::<T>().await?)
    }

    /// Fetch up to `max_bytes` of `remote:path`'s content from the `--rc-serve`
    /// object endpoint (a `Range` request), for file previews.
    pub async fn fetch_object(
        &self,
        remote: &str,
        path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, RcError> {
        // `--rc-serve` serves objects at `/[remote:]/path` (literal brackets).
        let mut url = reqwest::Url::parse(&self.base_url)
            .map_err(|e| RcError::Status { status: 0, message: e.to_string() })?;
        url.set_path(&format!("[{remote}:]/{path}"));
        let resp = self
            .http
            .get(url)
            .basic_auth(&self.user, Some(&self.pass))
            .header(reqwest::header::RANGE, format!("bytes=0-{}", max_bytes.saturating_sub(1)))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
            let body = resp.text().await.unwrap_or_default();
            return Err(RcError::Status { status: status.as_u16(), message: rc_error_message(&body) });
        }
        Ok(resp.bytes().await?.to_vec())
    }

    /// Liveness check (`rc/noop`).
    pub async fn noop(&self) -> Result<(), RcError> {
        let _: Value = self.call("rc/noop", &json!({})).await?;
        Ok(())
    }

    /// Names of configured remotes.
    pub async fn list_remotes(&self) -> Result<Vec<String>, RcError> {
        #[derive(serde::Deserialize)]
        struct Remotes {
            remotes: Vec<String>,
        }
        let r: Remotes = self.call("config/listremotes", &json!({})).await?;
        Ok(r.remotes)
    }

    /// Configured remotes with their backend types, sorted by name.
    pub async fn remotes(&self) -> Result<Vec<RemoteInfo>, RcError> {
        let dump: std::collections::BTreeMap<String, Value> =
            self.call("config/dump", &json!({})).await?;
        let mut out: Vec<RemoteInfo> = dump
            .into_iter()
            .map(|(name, cfg)| {
                let kind = cfg.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
                RemoteInfo { name, kind }
            })
            .collect();
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(out)
    }

    /// Delete a configured remote (`config/delete`).
    pub async fn config_delete(&self, name: &str) -> Result<(), RcError> {
        let _: Value = self.call("config/delete", &json!({ "name": name })).await?;
        Ok(())
    }

    /// Stop rclone's local OAuth callback webserver (`config/oauthstop`).
    /// Best-effort: errors when no auth runs or the method is absent (older
    /// rclone), so it logs at debug via `call_inner` instead of `call`'s warn.
    pub async fn config_oauth_stop(&self) -> Result<(), RcError> {
        let result = self.call_inner::<Value>("config/oauthstop", &json!({})).await;
        if let Err(e) = &result {
            tracing::debug!(error = %e, "config/oauthstop best-effort failed");
        }
        result.map(|_| ())
    }

    /// All configurable backends and their option schemas (`config/providers`).
    pub async fn config_providers(&self) -> Result<Vec<Provider>, RcError> {
        #[derive(Deserialize)]
        struct Providers {
            providers: Vec<Provider>,
        }
        let r: Providers = self.call("config/providers", &json!({})).await?;
        Ok(r.providers)
    }

    /// rclone's resolved config/cache/temp paths (`config/paths`).
    pub async fn config_paths(&self) -> Result<ConfigPaths, RcError> {
        self.call("config/paths", &json!({})).await
    }

    /// The stored parameters of a configured remote (`config/get`), for editing.
    pub async fn config_get(&self, name: &str) -> Result<serde_json::Map<String, Value>, RcError> {
        self.call("config/get", &json!({ "name": name })).await
    }

    /// One step of interactive remote creation (`config/create`). `parameters`
    /// pre-fills known answers; `opt` drives the state machine
    /// (`state`/`result`/`continue`/`obscure`/`nonInteractive`).
    pub async fn config_create(
        &self,
        name: &str,
        kind: &str,
        parameters: Value,
        opt: Value,
    ) -> Result<ConfigStep, RcError> {
        self.call(
            "config/create",
            &json!({ "name": name, "type": kind, "parameters": parameters, "opt": opt }),
        )
        .await
    }

    /// One step of interactive remote editing (`config/update`).
    pub async fn config_update(
        &self,
        name: &str,
        parameters: Value,
        opt: Value,
    ) -> Result<ConfigStep, RcError> {
        self.call("config/update", &json!({ "name": name, "parameters": parameters, "opt": opt }))
            .await
    }

    /// List one directory level. `fs` is the remote (e.g. `"drive:"`), `remote`
    /// is the path within it (empty for the root).
    pub async fn list(&self, fs: &str, remote: &str) -> Result<Vec<Entry>, RcError> {
        #[derive(Deserialize)]
        struct Listing {
            list: Vec<Entry>,
        }
        let r: Listing = self.call("operations/list", &json!({ "fs": fs, "remote": remote })).await?;
        Ok(r.list)
    }

    /// Run `method` as an async rclone job in stats group `group`, returning the
    /// job id immediately. Progress is then read via [`stats`](Self::stats) and
    /// state via [`job_status`](Self::job_status).
    pub async fn call_async(
        &self,
        method: &str,
        mut params: Value,
        group: &str,
    ) -> Result<u64, RcError> {
        params["_async"] = Value::Bool(true);
        params["_group"] = Value::String(group.to_string());
        #[derive(Deserialize)]
        struct JobId {
            jobid: u64,
        }
        let r: JobId = self.call(method, &params).await?;
        Ok(r.jobid)
    }

    pub async fn job_status(&self, jobid: u64) -> Result<JobStatus, RcError> {
        self.call("job/status", &json!({ "jobid": jobid })).await
    }

    pub async fn job_stop(&self, jobid: u64) -> Result<(), RcError> {
        let _: Value = self.call("job/stop", &json!({ "jobid": jobid })).await?;
        Ok(())
    }

    /// Transfer stats for a job's stats group.
    pub async fn stats(&self, group: &str) -> Result<Stats, RcError> {
        self.call("core/stats", &json!({ "group": group })).await
    }

    /// Ask the daemon to terminate.
    pub async fn quit(&self) -> Result<(), RcError> {
        let _: Value = self.call("core/quit", &json!({})).await?;
        Ok(())
    }
}
