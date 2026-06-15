//! File operations (new/rename/copy/paste/delete/upload) and the job queue.

use super::*;

impl Workspace {
    pub(crate) fn new_folder(&mut self, _: &NewFolder, _window: &mut Window, cx: &mut Context<Self>) {
        self.begin_new_folder(cx);
    }

    /// Move (or copy, if `copy`) the dragged entry — or the whole selection it
    /// belongs to — into `dst_remote:dst_dir`. Works within a remote (folder /
    /// breadcrumb drop) or across remotes (drop on the sidebar). One job per item.
    pub(crate) fn drop_into(
        &mut self,
        dragged: &DraggedEntry,
        dst_remote: String,
        dst_dir: String,
        copy: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(src_remote) = self.open_remote.clone() else {
            return;
        };
        let same = src_remote == dst_remote;
        let items: Vec<(String, String, bool)> = if self.selected.contains(&dragged.path) {
            self.selected_entries().into_iter().map(|e| (e.path, e.name, e.is_dir)).collect()
        } else {
            vec![(dragged.path.clone(), dragged.name.clone(), dragged.is_dir)]
        };
        let mode = if copy { TransferMode::Copy } else { TransferMode::Move };
        for (path, name, is_dir) in items {
            // Within the same remote: skip if already there or a folder onto itself.
            if same && parent_of(&path) == dst_dir {
                continue;
            }
            if same && is_dir && (dst_dir == path || dst_dir.starts_with(&format!("{path}/"))) {
                continue;
            }
            let dst_path = join_path(&dst_dir, &name);
            let verb = if copy { "Copy" } else { "Move" };
            let source = JobTarget::new(name.clone(), src_remote.clone(), path.clone());
            let destination = JobTarget::new(name, dst_remote.clone(), dst_path.clone());
            let command = rclone_cmd(
                mode.cli_verb(is_dir),
                &[&format!("{src_remote}:{path}"), &format!("{dst_remote}:{dst_path}")],
            );
            let (from_remote, into_remote, into_dir) =
                (src_remote.clone(), dst_remote.clone(), dst_dir.clone());
            let service = self.service.clone();
            self.spawn_job(verb, vec![source, destination], command, true, cx, move |group| {
                let (service, from_remote, path, into_remote, into_dir) = (
                    service.clone(),
                    from_remote.clone(),
                    path.clone(),
                    into_remote.clone(),
                    into_dir.clone(),
                );
                async move { service.paste(from_remote, path, is_dir, into_remote, into_dir, mode, group).await }
            });
        }
    }

    pub(crate) fn begin_new_folder(&mut self, cx: &mut Context<Self>) {
        if self.open_remote.is_none() {
            return;
        }
        self.begin_edit("", "Folder name", true, None, |this, name, cx| this.create_folder(name, cx), cx);
    }

    pub(crate) fn new_file(&mut self, _: &NewFile, _window: &mut Window, cx: &mut Context<Self>) {
        self.begin_upload(cx);
    }

    pub(crate) fn rename(&mut self, _: &Rename, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pane != Pane::Explorer {
            return;
        }
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        if let Some(entry) = self.entries().get(self.entry_sel).cloned() {
            self.begin_rename(remote, entry, cx);
        }
    }

    pub(crate) fn begin_rename(&mut self, remote: String, entry: Entry, cx: &mut Context<Self>) {
        let target = entry.path.clone();
        self.begin_edit(
            entry.name.clone(),
            "",
            entry.is_dir,
            Some(target),
            move |this, name, cx| this.rename_entry(remote, entry, name, cx),
            cx,
        );
    }

    fn rename_entry(&mut self, remote: String, entry: Entry, new_name: String, cx: &mut Context<Self>) {
        if new_name == entry.name {
            return;
        }
        let to = join_path(parent_of(&entry.path), &new_name);
        self.pending_select = Some(new_name.clone());
        let (from, is_dir) = (entry.path.clone(), entry.is_dir);
        let source = JobTarget::new(entry.name, remote.clone(), from.clone());
        let destination = JobTarget::new(new_name, remote.clone(), to.clone());
        let command = rclone_cmd(
            TransferMode::Move.cli_verb(is_dir),
            &[&format!("{remote}:{from}"), &format!("{remote}:{to}")],
        );
        let service = self.service.clone();
        self.spawn_job(
            "Rename",
            vec![source, destination],
            command,
            true,
            cx,
            move |group| {
                let (service, remote, from, to) =
                    (service.clone(), remote.clone(), from.clone(), to.clone());
                async move { service.move_to(remote, from, to, is_dir, group).await }
            },
        );
    }

    fn create_folder(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        let path = join_path(&self.path, &name);
        self.pending_select = Some(name.clone());
        let folder = JobTarget::new(name, remote.clone(), path.clone());
        let command = rclone_cmd("mkdir", &[&format!("{remote}:{path}")]);
        let service = self.service.clone();
        self.spawn_job("New folder", vec![folder], command, true, cx, move |group| {
            let (service, remote, path) = (service.clone(), remote.clone(), path.clone());
            async move { service.mkdir(remote, path, group).await }
        });
    }

    pub(crate) fn begin_upload(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        let dst_dir = self.path.clone();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                this.update(cx, |this, cx| {
                    for path in paths {
                        let is_dir = path.is_dir();
                        let local = path.to_string_lossy().into_owned();
                        let name = local.rsplit('/').next().unwrap_or(&local).to_string();
                        let (r, d) = (remote.clone(), dst_dir.clone());
                        let dst_path = join_path(&d, &name);
                        let cli = if is_dir { "copy" } else { "copyto" };
                        let command = rclone_cmd(cli, &[&local, &format!("{r}:{dst_path}")]);
                        // Local source has no remote location; only the destination is navigable.
                        let destination = JobTarget::new(name, r.clone(), dst_path);
                        let service = this.service.clone();
                        this.spawn_job("Upload", vec![destination], command, true, cx, move |group| {
                            let (service, local, r, d) =
                                (service.clone(), local.clone(), r.clone(), d.clone());
                            async move { service.upload(local, r, d, is_dir, group).await }
                        });
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn copy(&mut self, _: &CopyEntry, _window: &mut Window, cx: &mut Context<Self>) {
        self.set_clipboard(TransferMode::Copy, cx);
    }

    pub(crate) fn cut(&mut self, _: &CutEntry, _window: &mut Window, cx: &mut Context<Self>) {
        self.set_clipboard(TransferMode::Move, cx);
    }

    pub(crate) fn paste(&mut self, _: &PasteEntry, _window: &mut Window, cx: &mut Context<Self>) {
        self.paste_clipboard(cx);
    }

    pub(crate) fn delete(&mut self, _: &DeleteEntry, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pane == Pane::Explorer {
            self.request_delete_selected(cx);
        }
    }

    /// Confirm deleting the current selection, then enqueue one job per item.
    pub(crate) fn request_delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        let entries = self.selected_entries();
        let n = entries.len();
        let what = match entries.first() {
            Some(e) if n == 1 => {
                format!("\u{201c}{}\u{201d}", e.name)
            }
            _ => format!("{n} items"),
        };
        if n == 0 {
            return;
        }
        self.ask_confirm(
            if n == 1 { "Delete?".to_string() } else { format!("Delete {n} items?") },
            format!("{what} will be permanently deleted from {remote}. This cannot be undone."),
            "Delete",
            true,
            move |this, cx| {
                for entry in &entries {
                    this.delete_entry(remote.clone(), entry.clone(), cx);
                }
            },
            cx,
        );
    }

    /// Enqueue a single delete as an rclone job.
    fn delete_entry(&mut self, remote: String, entry: Entry, cx: &mut Context<Self>) {
        let (path, is_dir) = (entry.path.clone(), entry.is_dir);
        let item = JobTarget::new(entry.name, remote.clone(), path.clone());
        let command =
            rclone_cmd(if is_dir { "purge" } else { "deletefile" }, &[&format!("{remote}:{path}")]);
        let service = self.service.clone();
        self.spawn_job("Delete", vec![item], command, true, cx, move |group| {
            let (service, remote, path) = (service.clone(), remote.clone(), path.clone());
            async move { service.delete(remote, path, is_dir, group).await }
        });
    }

    /// Open a confirmation dialog; `action` runs only if the user confirms.

    pub(crate) fn download_selected(&mut self, cx: &mut Context<Self>) {
        for entry in self.selected_entries() {
            self.download_entry(&entry, cx);
        }
    }

    fn download_entry(&mut self, entry: &Entry, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        let dest = self.store.get().download_dir();
        let (path, is_dir) = (entry.path.clone(), entry.is_dir);
        let local = format!("{}/{}", dest.to_string_lossy(), entry.name);
        // Local destination has no remote location; only the source is navigable.
        let source = JobTarget::new(entry.name.clone(), remote.clone(), path.clone());
        let command =
            rclone_cmd(TransferMode::Copy.cli_verb(is_dir), &[&format!("{remote}:{path}"), &local]);
        let service = self.service.clone();
        self.spawn_job("Download", vec![source], command, false, cx, move |group| {
            let (service, remote, path, dest) =
                (service.clone(), remote.clone(), path.clone(), dest.clone());
            async move { service.download(remote, path, is_dir, dest, group).await }
        });
    }

    /// Mark the selection for a copy or cut.
    pub(crate) fn set_clipboard(&mut self, mode: TransferMode, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        if self.pane != Pane::Explorer {
            return;
        }
        let entries = self.selected_entries();
        if entries.is_empty() {
            return;
        }
        self.clipboard = Some(Clipboard { remote, entries, mode });
        cx.notify();
    }

    /// Paste every clipboard item into the open directory, one job each. A cut
    /// clears the clipboard once enqueued; a copy stays for repeated pastes.
    pub(crate) fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(clip) = self.clipboard.clone() else {
            return;
        };
        let Some(dst_remote) = self.open_remote.clone() else {
            return;
        };
        let dst_dir = self.path.clone();
        for entry in &clip.entries {
            // Same remote and same parent directory: nothing to do.
            if clip.remote == dst_remote && parent_of(&entry.path) == dst_dir {
                continue;
            }
            let verb = match clip.mode {
                TransferMode::Copy => "Copy",
                TransferMode::Move => "Move",
            };
            let (src_remote, src_path, is_dir, mode) =
                (clip.remote.clone(), entry.path.clone(), entry.is_dir, clip.mode);
            let dst_remote = dst_remote.clone();
            let dst_dir = dst_dir.clone();
            let dst_path = join_path(&dst_dir, &entry.name);
            let source = JobTarget::new(entry.name.clone(), src_remote.clone(), src_path.clone());
            let destination = JobTarget::new(entry.name.clone(), dst_remote.clone(), dst_path.clone());
            let command = rclone_cmd(
                mode.cli_verb(is_dir),
                &[&format!("{src_remote}:{src_path}"), &format!("{dst_remote}:{dst_path}")],
            );
            let service = self.service.clone();
            self.spawn_job(verb, vec![source, destination], command, true, cx, move |group| {
                let (service, src_remote, src_path, dst_remote, dst_dir) = (
                    service.clone(),
                    src_remote.clone(),
                    src_path.clone(),
                    dst_remote.clone(),
                    dst_dir.clone(),
                );
                async move { service.paste(src_remote, src_path, is_dir, dst_remote, dst_dir, mode, group).await }
            });
        }
        if matches!(clip.mode, TransferMode::Move) {
            self.clipboard = None;
        }
        cx.notify();
    }

    /// Push a tracked job; `run(group)` returns the submission future. `run` is
    /// `Fn` (re-runnable) so the job can be retried; it must clone its captures
    /// per call. `command` is the equivalent rclone CLI shown by the copy button.
    fn spawn_job<F, Fut>(
        &mut self,
        verb: impl Into<SharedString>,
        targets: Vec<JobTarget>,
        command: String,
        reload_on_done: bool,
        cx: &mut Context<Self>,
        run: F,
    ) where
        F: Fn(String) -> Fut + 'static,
        Fut: std::future::Future<Output = Result<u64, ServiceError>> + 'static,
    {
        let run: JobRun = Rc::new(move |group| Box::pin(run(group)));
        self.enqueue(verb.into(), targets, command, reload_on_done, run, cx);
    }

    /// Re-run a failed/cancelled job: drop the old row, enqueue a fresh one with
    /// the same operation.
    pub(crate) fn retry_job(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(job) = self.jobs.iter().find(|j| j.id == id) else {
            return;
        };
        let (verb, targets, command, reload, run) =
            (job.verb.clone(), job.targets.clone(), job.command.clone(), job.reload_on_done, job.run.clone());
        self.jobs.retain(|j| j.id != id);
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
        let id = self.job_seq;
        self.job_seq += 1;
        let group = format!("rspace/{id}");
        self.jobs.push(Job {
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

    /// Remove a single finished job from the list.
    pub(crate) fn clear_job(&mut self, id: usize, cx: &mut Context<Self>) {
        self.jobs.retain(|j| j.id != id);
        self.jobs_changed(cx);
    }

    /// Drop the transfers panel when no jobs remain, then notify.
    fn jobs_changed(&mut self, cx: &mut Context<Self>) {
        if self.jobs.is_empty() {
            self.jobs_open = false;
        }
        cx.notify();
    }

    /// Record the rclone job id (or the submission error) for job `id`.
    fn on_job_submitted(&mut self, id: usize, result: Result<u64, ServiceError>, cx: &mut Context<Self>) {
        if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
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

    /// Confirm, then cancel a running job (reuses [`ask_confirm`]).
    pub(crate) fn request_cancel_job(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(label) = self.jobs.iter().find(|j| j.id == id).map(|j| j.label()) else {
            return;
        };
        self.ask_confirm(
            "Cancel task?",
            format!("Stop \u{201c}{label}\u{201d}? Work already done is kept."),
            "Cancel task",
            true,
            move |this, cx| this.cancel_job(id, cx),
            cx,
        );
    }

    pub(crate) fn cancel_job(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(jobid) = self.jobs.iter().find(|j| j.id == id).and_then(|j| j.jobid) else {
            return;
        };
        let service = self.service.clone();
        cx.spawn(async move |this, cx| {
            let _ = service.job_stop(jobid).await;
            this.update(cx, |this, cx| {
                if let Some(j) = this.jobs.iter_mut().find(|j| j.id == id) {
                    j.done = true;
                    j.error.get_or_insert_with(|| "cancelled".into());
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Confirm, then drop finished jobs from the list (reuses [`ask_confirm`]).
    pub(crate) fn request_clear_finished(&mut self, cx: &mut Context<Self>) {
        let n = self.jobs.iter().filter(|j| j.done).count();
        if n == 0 {
            return;
        }
        self.ask_confirm(
            "Clear finished?",
            format!("Remove {n} finished task{} from the list.", if n == 1 { "" } else { "s" }),
            "Clear",
            false,
            |this, cx| this.clear_finished(cx),
            cx,
        );
    }

    fn clear_finished(&mut self, cx: &mut Context<Self>) {
        self.jobs.retain(|j| !j.done);
        self.jobs_changed(cx);
    }
}
