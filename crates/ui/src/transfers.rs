//! The transfer queue: a [`Jobs`] entity owning tracked rclone jobs, their
//! submission, polling, and lifecycle. File operations live on [`Workspace`]
//! and enqueue here via [`Jobs::spawn_job`].

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
}

impl JobTarget {
    pub(crate) fn new(name: impl Into<SharedString>, remote: String, path: String) -> Self {
        Self { name: name.into(), remote, path }
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
    pub(crate) error: Option<String>,
    pub(crate) bytes: u64,
    pub(crate) total: u64,
    pub(crate) speed: f64,
    pub(crate) transfers: u64,
    pub(crate) total_transfers: u64,
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
}

/// A successful job that should refresh the open listing.
pub(crate) enum JobsEvent {
    ReloadEntries,
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
                let active = match this.update(cx, |this, _| {
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
                        let mut reload = false;
                        if let Some(j) = this.items.iter_mut().find(|j| j.id == id) {
                            if let Some(s) = &stats {
                                j.bytes = s.bytes;
                                j.total = s.total_bytes;
                                j.speed = s.speed;
                                j.transfers = s.transfers;
                                j.total_transfers = s.total_transfers;
                                j.elapsed_ms = (s.elapsed_time * 1000.0) as u64;
                            }
                            if let Some(st) = &status {
                                if st.finished && !j.done {
                                    j.done = true;
                                    if st.duration > 0.0 {
                                        j.elapsed_ms = (st.duration * 1000.0) as u64;
                                    }
                                    if st.success {
                                        reload = j.reload_on_done;
                                        tracing::debug!(job = %j.label(), elapsed_ms = j.elapsed_ms, "job done");
                                    } else {
                                        let msg = if st.error.is_empty() {
                                            "failed".to_string()
                                        } else {
                                            st.error.clone()
                                        };
                                        tracing::warn!(job = %j.label(), elapsed_ms = j.elapsed_ms, error = %msg, "job failed");
                                        j.error = Some(msg);
                                    }
                                }
                            }
                        }
                        if reload {
                            cx.emit(JobsEvent::ReloadEntries);
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
        self.items.push(Job {
            id,
            group: group.clone(),
            jobid: None,
            verb,
            targets,
            done: false,
            error: None,
            bytes: 0,
            total: 0,
            speed: 0.0,
            transfers: 0,
            total_transfers: 0,
            reload_on_done,
            elapsed_ms: 0,
            command,
            run: run.clone(),
        });
        cx.spawn(async move |this, cx| {
            let result = run(group).await;
            this.update(cx, |this, cx| this.on_job_submitted(id, result, cx)).ok();
        })
        .detach();
    }

    fn on_job_submitted(&mut self, id: usize, result: Result<u64, ServiceError>, cx: &mut Context<Self>) {
        if let Some(j) = self.items.iter_mut().find(|j| j.id == id) {
            match result {
                Ok(jobid) => j.jobid = Some(jobid),
                Err(e) => {
                    j.done = true;
                    j.error = Some(e.to_string());
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn clear_job(&mut self, id: usize, cx: &mut Context<Self>) {
        self.items.retain(|j| j.id != id);
        cx.notify();
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
                    j.error.get_or_insert_with(|| "cancelled".into());
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
