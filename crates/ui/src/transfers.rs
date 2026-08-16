//! The transfer queue: a [`Jobs`] entity owning tracked rclone jobs, their
//! submission, polling, and lifecycle. File operations live on [`Workspace`]
//! and enqueue here via [`Jobs::spawn_job`]. Jobs are in-memory only — closing
//! the app drops them with the daemon.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;

use gpui::EventEmitter;

use super::*;

/// A navigable endpoint of a job (a source or destination): shown by name,
/// clicked to reveal it in the explorer.
#[derive(Clone)]
pub(crate) struct JobTarget {
    pub(crate) name: SharedString,
    pub(crate) remote: String,
    pub(crate) path: String,
    pub(crate) is_dir: bool,
}

impl JobTarget {
    pub(crate) fn new(name: impl Into<SharedString>, remote: String, path: String, is_dir: bool) -> Self {
        Self { name: name.into(), remote, path, is_dir }
    }
}

/// A re-runnable job submission: takes the stats group, returns the rclone job
/// id. `Rc` so a finished/failed [`Job`] stays `Clone` and can be retried.
pub(crate) type JobRun =
    Rc<dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<u64, ServiceError>>>>>;

/// A tracked rclone job (download/copy/…). State mirrors rclone's job + stats.
#[derive(Clone)]
pub(crate) struct Job {
    pub(crate) id: usize,
    pub(crate) group: String,
    pub(crate) jobid: Option<u64>,
    pub(crate) verb: SharedString,
    pub(crate) targets: Vec<JobTarget>,
    pub(crate) done: bool,
    /// User-stopped: shown as a neutral "Cancelled", not a failure.
    pub(crate) cancelled: bool,
    pub(crate) error: Option<String>,
    pub(crate) bytes: u64,
    pub(crate) total: u64,
    pub(crate) speed: f64,
    pub(crate) transfers: u64,
    pub(crate) total_transfers: u64,
    /// Files + directories removed, for ops that transfer no bytes.
    pub(crate) removed: u64,
    /// Refresh the open listing when this job succeeds (paste changed a remote).
    pub(crate) reload_on_done: bool,
    /// Elapsed from rclone: live `core/stats.elapsedTime`, then `job/status.duration`.
    pub(crate) elapsed_ms: u64,
    /// Equivalent rclone CLI command, for the row's copy button.
    pub(crate) command: String,
    /// Re-runs the operation; used by the failed-row retry button.
    pub(crate) run: JobRun,
}

impl Job {
    /// Plain-text summary for logs, e.g. `Copy report.pdf → archive`.
    pub(crate) fn label(&self) -> String {
        let names: Vec<&str> = self.targets.iter().map(|t| t.name.as_ref()).collect();
        format!("{} {}", self.verb, names.join(" → "))
    }

    /// `(remote, dir)` listings this job may have changed: each target's parent
    /// (where it appears/disappears) plus the target itself when it's a directory
    /// (its contents change — paste/sync into it). Deduped.
    fn affected_dirs(&self) -> Vec<(String, String)> {
        let mut dirs: Vec<(String, String)> = Vec::new();
        let mut push = |remote: &str, dir: String| {
            let key = (remote.to_string(), dir);
            if !dirs.contains(&key) {
                dirs.push(key);
            }
        };
        for t in &self.targets {
            push(&t.remote, rspace_rclone_rc::split_parent(&t.path).0);
            if t.is_dir {
                push(&t.remote, t.path.clone());
            }
        }
        dirs
    }
}

pub(crate) enum JobsEvent {
    /// A successful mutating job: these `(remote, dir)` listings may have changed,
    /// so any view showing one should refetch (and others drop their stale cache).
    Invalidate(Vec<(String, String)>),
    /// A job finished: notify (success toasts auto-dismiss; failures stay until
    /// dismissed, showing rclone's error).
    Finished { verb: SharedString, label: SharedString, ok: bool, error: Option<SharedString> },
}

pub(crate) struct Jobs {
    service: Service,
    items: Vec<Job>,
    seq: usize,
}

impl EventEmitter<JobsEvent> for Jobs {}

impl Jobs {
    pub(crate) fn new(service: Service) -> Self {
        Self { service, items: Vec::new(), seq: 0 }
    }

    pub(crate) fn items(&self) -> &[Job] {
        &self.items
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(crate) fn has_finished(&self) -> bool {
        self.items.iter().any(|j| j.done)
    }

    pub(crate) fn finished_count(&self) -> usize {
        self.items.iter().filter(|j| j.done).count()
    }

    pub(crate) fn label_of(&self, id: usize) -> Option<String> {
        self.items.iter().find(|j| j.id == id).map(|j| j.label())
    }

    /// Poll rclone every second for the state and progress of active jobs.
    pub(crate) fn start_polling(&self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                // The daemon owns the task list: drop any job it no longer tracks
                // (finished + expired, or gone after a daemon restart). A failed
                // query leaves the list alone so a transient blip doesn't wipe it.
                let live = service
                    .job_list()
                    .await
                    .ok()
                    .map(|ids| ids.into_iter().collect::<std::collections::HashSet<u64>>());
                let active = match this.update(cx, |this, cx| {
                    if let Some(live) = &live {
                        let before = this.items.len();
                        this.items.retain(|j| j.jobid.is_none_or(|id| live.contains(&id)));
                        if this.items.len() != before {
                            cx.notify();
                        }
                    }
                    this.items
                        .iter()
                        .filter(|j| !j.done && j.jobid.is_some())
                        .map(|j| (j.id, j.group.clone(), j.jobid.unwrap()))
                        .collect::<Vec<_>>()
                }) {
                    Ok(active) => active,
                    Err(_) => return,
                };
                for (id, group, jobid) in active {
                    let status = service.job_status(jobid).await.ok();
                    let stats = service.stats(group).await.ok();
                    let alive = this.update(cx, |this, cx| {
                        // Dirs to invalidate once the borrow on `j` ends (empty = nothing).
                        let mut invalidate: Vec<(String, String)> = Vec::new();
                        // Set when the job finishes this tick, to emit after the borrow.
                        let mut finished: Option<(SharedString, SharedString, bool, Option<SharedString>)> = None;
                        if let Some(j) = this.items.iter_mut().find(|j| j.id == id) {
                            if let Some(s) = &stats {
                                j.bytes = s.bytes;
                                j.total = s.total_bytes;
                                j.speed = s.speed;
                                j.transfers = s.transfers;
                                j.total_transfers = s.total_transfers;
                                j.removed = s.deletes + s.deleted_dirs;
                                j.elapsed_ms = (s.elapsed_time * 1000.0) as u64;
                            }
                            if let Some(st) = &status {
                                if st.finished && !j.done {
                                    j.done = true;
                                    if st.duration > 0.0 {
                                        j.elapsed_ms = (st.duration * 1000.0) as u64;
                                    }
                                    if st.success {
                                        if j.reload_on_done {
                                            invalidate = j.affected_dirs();
                                        }
                                        tracing::debug!(job = %j.label(), elapsed_ms = j.elapsed_ms, "job done");
                                    } else {
                                        let msg = if st.error.is_empty() {
                                            "failed".to_string()
                                        } else {
                                            st.error.clone()
                                        };
                                        tracing::warn!(job = %j.label(), command = %j.command, elapsed_ms = j.elapsed_ms, error = %msg, "job failed");
                                        j.error = Some(msg);
                                    }
                                    let err = if st.success { None } else { j.error.clone() };
                                    finished = Some((j.verb.clone(), j.label().into(), st.success, err.map(Into::into)));
                                }
                            }
                        }
                        if let Some((verb, label, ok, error)) = finished {
                            cx.emit(JobsEvent::Finished { verb, label, ok, error });
                        }
                        if !invalidate.is_empty() {
                            cx.emit(JobsEvent::Invalidate(invalidate));
                        }
                        cx.notify();
                    });
                    if alive.is_err() {
                        return;
                    }
                }
            }
        })
        .detach();
    }

    /// Push a tracked job. `run` is `Fn` (not `FnOnce`) so a failed job can be
    /// retried; it must clone its captures per call.
    pub(crate) fn spawn_job<F, Fut>(
        &mut self,
        verb: impl Into<SharedString>,
        targets: Vec<JobTarget>,
        command: String,
        reload_on_done: bool,
        cx: &mut Context<Self>,
        run: F,
    ) where
        F: Fn(String) -> Fut + 'static,
        Fut: Future<Output = Result<u64, ServiceError>> + 'static,
    {
        let run: JobRun = Rc::new(move |group| Box::pin(run(group)));
        self.enqueue(verb.into(), targets, command, reload_on_done, run, cx);
    }

    /// Re-run a failed/cancelled job: drop the old row, enqueue a fresh one.
    pub(crate) fn retry(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(job) = self.items.iter().find(|j| j.id == id) else {
            return;
        };
        let (verb, targets, command, reload, run) =
            (job.verb.clone(), job.targets.clone(), job.command.clone(), job.reload_on_done, job.run.clone());
        self.items.retain(|j| j.id != id);
        self.enqueue(verb, targets, command, reload, run, cx);
        cx.notify();
    }

    fn enqueue(
        &mut self,
        verb: SharedString,
        targets: Vec<JobTarget>,
        command: String,
        reload_on_done: bool,
        run: JobRun,
        cx: &mut Context<Self>,
    ) {
        let id = self.seq;
        self.seq += 1;
        let group = format!("rspace/{id}");
        tracing::info!(%group, command = %command, "job enqueued");
        self.items.push(Job {
            id,
            group: group.clone(),
            jobid: None,
            verb,
            targets,
            done: false,
            cancelled: false,
            error: None,
            bytes: 0,
            total: 0,
            speed: 0.0,
            transfers: 0,
            total_transfers: 0,
            removed: 0,
            reload_on_done,
            elapsed_ms: 0,
            command,
            run: run.clone(),
        });
        self.trim_history();
        cx.spawn(async move |this, cx| {
            let result = run(group).await;
            this.update(cx, |this, cx| this.on_job_submitted(id, result, cx)).ok();
        })
        .detach();
    }

    fn on_job_submitted(&mut self, id: usize, result: Result<u64, ServiceError>, cx: &mut Context<Self>) {
        // A job that fails to start never reaches the poll loop, so settle it here.
        let mut failed: Option<(SharedString, SharedString, SharedString)> = None;
        if let Some(j) = self.items.iter_mut().find(|j| j.id == id) {
            match result {
                Ok(jobid) => j.jobid = Some(jobid),
                Err(e) => {
                    let err = e.to_string();
                    tracing::warn!(command = %j.command, error = %err, "job submit failed");
                    j.done = true;
                    j.error = Some(err.clone());
                    failed = Some((j.verb.clone(), j.label().into(), err.into()));
                }
            }
        }
        if let Some((verb, label, err)) = failed {
            cx.emit(JobsEvent::Finished { verb, label, ok: false, error: Some(err) });
        }
        cx.notify();
    }

    pub(crate) fn clear_job(&mut self, id: usize, cx: &mut Context<Self>) {
        self.items.retain(|j| j.id != id);
        cx.notify();
    }

    /// Bound the finished-job history (oldest dropped first) so the task list and
    /// its memory stay O(1) over a long session. Active jobs are never dropped.
    fn trim_history(&mut self) {
        const MAX_FINISHED: usize = 200;
        let finished = self.items.iter().filter(|j| j.done).count();
        let mut over = finished.saturating_sub(MAX_FINISHED);
        if over > 0 {
            self.items.retain(|j| {
                if j.done && over > 0 {
                    over -= 1;
                    false
                } else {
                    true
                }
            });
        }
    }

    pub(crate) fn clear_finished(&mut self, cx: &mut Context<Self>) {
        self.items.retain(|j| !j.done);
        cx.notify();
    }

    pub(crate) fn cancel(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(jobid) = self.items.iter().find(|j| j.id == id).and_then(|j| j.jobid) else {
            return;
        };
        let service = self.service.clone();
        cx.spawn(async move |this, cx| {
            let _ = service.job_stop(jobid).await;
            this.update(cx, |this, cx| {
                if let Some(j) = this.items.iter_mut().find(|j| j.id == id) {
                    j.done = true;
                    j.cancelled = true;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
