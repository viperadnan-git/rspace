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
        let items: Vec<(String, String, bool)> = if self.explorer.read(cx).is_selected(&dragged.path) {
            self.selected_entries(cx).into_iter().map(|e| (e.path, e.name, e.is_dir)).collect()
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
            let source = JobTarget::new(name.clone(), src_remote.clone(), path.clone(), is_dir);
            let destination = JobTarget::new(name, dst_remote.clone(), dst_path.clone(), is_dir);
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

    /// Run a registry [`Operation`] from resolved `args` as a tracked job — the
    /// shared execution path for the command palette (and, later, the context
    /// menu). Destructive ops confirm first; display targets and command string
    /// derive from the same `args`.
    pub(crate) fn run_operation(&mut self, op: Operation, args: Vec<ArgValue>, cx: &mut Context<Self>) {
        if op.destructive() {
            let target = args
                .iter()
                .find_map(|a| match a {
                    ArgValue::Path { remote, path, .. } if path.is_empty() => Some(format!("{remote}:")),
                    ArgValue::Path { remote, path, .. } => Some(format!("{remote}:{path}")),
                    ArgValue::Name(_) => None,
                })
                .unwrap_or_default();
            let note = match op {
                Operation::Cleanup => "Removes trash and old versions on the remote.",
                _ => "This permanently removes it; files are not recoverable.",
            };
            let label = op.label();
            self.ask_confirm(
                format!("{label}?"),
                format!("{label} \u{201c}{target}\u{201d}. {note}"),
                label,
                true,
                move |this, cx| this.spawn_operation(op, args, cx),
                cx,
            );
        } else {
            self.spawn_operation(op, args, cx);
        }
    }

    /// Run a read-only [`InfoOp`] and show its result (a toast; a public link is
    /// copied to the clipboard). Shared by the palette and the preview pane.
    pub(crate) fn run_info_op(&mut self, op: InfoOp, args: Vec<ArgValue>, cx: &mut Context<Self>) {
        let Some((method, params)) = op.build(&args) else {
            return;
        };
        let remote = info_remote(&args);
        let path = info_path(&args);
        // A pending spinner toast that resolves into the result (promise-toast).
        let toast = self.toast_pending(info_pending_label(op, &args), cx);
        let service = self.service.clone();
        cx.spawn(async move |this, cx| {
            let res = service.query(method, params).await;
            this.update(cx, |this, cx| {
                let body = match res {
                    Ok(v) => match op.parse(&v) {
                        Some(result) => this.info_result_body(&remote, &path, result, cx),
                        None => ToastBody::Message {
                            message: format!("{}: nothing to show", op.label()).into(),
                            danger: false,
                        },
                    },
                    Err(e) => ToastBody::Message {
                        message: format!("{} failed: {e}", op.label()).into(),
                        danger: true,
                    },
                };
                // The result card stays until dismissed; a transient error or
                // "nothing to show" auto-dismisses like any other message.
                let auto_dismiss = !matches!(body, ToastBody::Info { .. });
                this.resolve_toast(toast, body, auto_dismiss, cx);
            })
            .ok();
        })
        .detach();
    }

    fn info_result_body(&mut self, remote: &str, path: &str, result: InfoResult, cx: &mut Context<Self>) -> ToastBody {
        let part = |o: Option<i64>| o.map(human_size).unwrap_or_else(|| "?".into());
        let full = format!("{remote}:{path}");
        match result {
            // About is about the whole remote: icon + name as the title, no path.
            InfoResult::Quota { used, total, free } => ToastBody::Info {
                icon: Some(self.remote_icon_for(remote)),
                title: remote.to_string().into(),
                value: Some(format!("{} of {} used", part(used), part(total)).into()),
                detail: Some(format!("{} free", part(free)).into()),
            },
            InfoResult::Size { count, bytes } => ToastBody::Info {
                icon: None,
                title: "Size".into(),
                value: Some(full.into()),
                detail: Some(format!("{count} items \u{b7} {}", human_size(bytes)).into()),
            },
            InfoResult::Stat { name, bytes, is_dir } => ToastBody::Info {
                icon: Some(if is_dir { "icons/folder.svg" } else { "icons/file.svg" }),
                title: name.into(),
                value: Some(full.into()),
                detail: Some(if is_dir { "Folder".to_string() } else { human_size(bytes) }.into()),
            },
            InfoResult::Link(url) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(url.clone()));
                ToastBody::Info {
                    icon: None,
                    title: "Public link".into(),
                    value: Some(url.into()),
                    detail: Some("Copied to clipboard".into()),
                }
            }
        }
    }

    fn remote_icon_for(&self, name: &str) -> &'static str {
        self.remotes.iter().find(|r| r.name == name).map_or("icons/cloud.svg", |r| remote_icon(&r.kind))
    }

    fn spawn_operation(&mut self, op: Operation, args: Vec<ArgValue>, cx: &mut Context<Self>) {
        let is_dir = matches!(args.first(), Some(ArgValue::Path { is_dir, .. }) if *is_dir);
        let targets: Vec<JobTarget> = args
            .iter()
            .filter_map(|a| match a {
                ArgValue::Path { remote, path, is_dir, .. } => {
                    let name = path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(remote);
                    Some(JobTarget::new(name.to_string(), remote.clone(), path.clone(), *is_dir))
                }
                ArgValue::Name(_) => None,
            })
            .collect();
        let parts: Vec<String> = args
            .iter()
            .map(|a| match a {
                ArgValue::Path { remote, path, .. } => format!("{remote}:{path}"),
                ArgValue::Name(n) => n.clone(),
            })
            .collect();
        let command = rclone_cmd(op.cli_verb(is_dir), &parts.iter().map(String::as_str).collect::<Vec<_>>());
        let service = self.service.clone();
        self.spawn_job(op.label(), targets, command, true, cx, move |group| {
            let (service, args) = (service.clone(), args.clone());
            async move { service.run_operation(op, args, group).await }
        });
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
        if let Some(entry) = self.explorer.read(cx).focused_entry() {
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
        self.explorer.update(cx, |e, _| e.set_pending(new_name.clone()));
        let (from, is_dir) = (entry.path.clone(), entry.is_dir);
        let source = JobTarget::new(entry.name, remote.clone(), from.clone(), is_dir);
        let destination = JobTarget::new(new_name.clone(), remote.clone(), to.clone(), is_dir);
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
                let (service, remote, from, new_name) =
                    (service.clone(), remote.clone(), from.clone(), new_name.clone());
                async move { service.move_to(remote, from, new_name, is_dir, group).await }
            },
        );
    }

    fn create_folder(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        let path = join_path(&self.path, &name);
        self.explorer.update(cx, |e, _| e.set_pending(name.clone()));
        let folder = JobTarget::new(name, remote.clone(), path.clone(), true);
        let command = rclone_cmd("mkdir", &[&format!("{remote}:{path}")]);
        let service = self.service.clone();
        self.spawn_job("New folder", vec![folder], command, true, cx, move |group| {
            let (service, remote, path) = (service.clone(), remote.clone(), path.clone());
            async move { service.mkdir(remote, path, group).await }
        });
    }

    pub(crate) fn begin_upload(&mut self, cx: &mut Context<Self>) {
        if self.open_remote.is_none() {
            return;
        }
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                this.update(cx, |this, cx| this.upload_paths(paths, cx)).ok();
            }
        })
        .detach();
    }

    pub(crate) fn upload_paths(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        let dst_dir = self.path.clone();
        for path in paths {
            let is_dir = path.is_dir();
            let local = path.to_string_lossy().into_owned();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| local.clone());
            let (r, d) = (remote.clone(), dst_dir.clone());
            let dst_path = join_path(&d, &name);
            let cli = if is_dir { "copy" } else { "copyto" };
            let command = rclone_cmd(cli, &[&local, &format!("{r}:{dst_path}")]);
            // Local source has no remote location; only the destination is navigable.
            let destination = JobTarget::new(name, r.clone(), dst_path, is_dir);
            let service = self.service.clone();
            self.spawn_job("Upload", vec![destination], command, true, cx, move |group| {
                let (service, local, r, d) = (service.clone(), local.clone(), r.clone(), d.clone());
                async move { service.upload(local, r, d, is_dir, group).await }
            });
        }
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

    pub(crate) fn request_delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        let entries = self.selected_entries(cx);
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

    fn delete_entry(&mut self, remote: String, entry: Entry, cx: &mut Context<Self>) {
        let (path, is_dir) = (entry.path.clone(), entry.is_dir);
        let item = JobTarget::new(entry.name, remote.clone(), path.clone(), is_dir);
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
        for entry in self.selected_entries(cx) {
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
        let source = JobTarget::new(entry.name.clone(), remote.clone(), path.clone(), is_dir);
        let command =
            rclone_cmd(TransferMode::Copy.cli_verb(is_dir), &[&format!("{remote}:{path}"), &local]);
        let service = self.service.clone();
        self.spawn_job("Download", vec![source], command, false, cx, move |group| {
            let (service, remote, path, dest) =
                (service.clone(), remote.clone(), path.clone(), dest.clone());
            async move { service.download(remote, path, is_dir, dest, group).await }
        });
    }

    pub(crate) fn set_clipboard(&mut self, mode: TransferMode, cx: &mut Context<Self>) {
        let Some(remote) = self.open_remote.clone() else {
            return;
        };
        if self.pane != Pane::Explorer {
            return;
        }
        let entries = self.selected_entries(cx);
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
            let source = JobTarget::new(entry.name.clone(), src_remote.clone(), src_path.clone(), is_dir);
            let destination = JobTarget::new(entry.name.clone(), dst_remote.clone(), dst_path.clone(), is_dir);
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

    /// Enqueue a tracked job on the [`Jobs`] entity. Thin delegate so the file
    /// operations above don't reach into the entity directly.
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
        self.jobs.update(cx, move |jobs, cx| {
            jobs.spawn_job(verb, targets, command, reload_on_done, cx, run)
        });
    }

    pub(crate) fn retry_job(&mut self, id: usize, cx: &mut Context<Self>) {
        self.jobs.update(cx, |jobs, cx| jobs.retry(id, cx));
    }

    pub(crate) fn clear_job(&mut self, id: usize, cx: &mut Context<Self>) {
        self.jobs.update(cx, |jobs, cx| jobs.clear_job(id, cx));
        if self.jobs.read(cx).is_empty() {
            self.jobs_open = false;
        }
        cx.notify();
    }

    pub(crate) fn request_cancel_job(&mut self, id: usize, cx: &mut Context<Self>) {
        let Some(label) = self.jobs.read(cx).label_of(id) else {
            return;
        };
        self.ask_confirm(
            "Cancel task?",
            format!("Stop \u{201c}{label}\u{201d}? Work already done is kept."),
            "Cancel task",
            true,
            move |this, cx| this.jobs.update(cx, |jobs, cx| jobs.cancel(id, cx)),
            cx,
        );
    }

    pub(crate) fn request_clear_finished(&mut self, cx: &mut Context<Self>) {
        let n = self.jobs.read(cx).finished_count();
        if n == 0 {
            return;
        }
        self.ask_confirm(
            "Clear finished?",
            format!("Remove {n} finished task{} from the list.", if n == 1 { "" } else { "s" }),
            "Clear",
            false,
            |this, cx| {
                this.jobs.update(cx, |jobs, cx| jobs.clear_finished(cx));
                if this.jobs.read(cx).is_empty() {
                    this.jobs_open = false;
                }
            },
            cx,
        );
    }
}

fn info_remote(args: &[ArgValue]) -> String {
    args.iter()
        .find_map(|a| match a {
            ArgValue::Path { remote, .. } => Some(remote.clone()),
            ArgValue::Name(_) => None,
        })
        .unwrap_or_default()
}

/// The path an info op targets (its first path arg); empty for a remote root.
fn info_path(args: &[ArgValue]) -> String {
    args.iter()
        .find_map(|a| match a {
            ArgValue::Path { path, .. } => Some(path.clone()),
            ArgValue::Name(_) => None,
        })
        .unwrap_or_default()
}

/// Spinner label shown while an info op runs, e.g. "Calculating size gdrive:photos…".
fn info_pending_label(op: InfoOp, args: &[ArgValue]) -> String {
    let target = args.iter().find_map(|a| match a {
        ArgValue::Path { remote, path, .. } if path.is_empty() => Some(format!("{remote}:")),
        ArgValue::Path { remote, path, .. } => Some(format!("{remote}:{path}")),
        ArgValue::Name(_) => None,
    });
    let verb = match op {
        InfoOp::Size => "Calculating size",
        InfoOp::About => "Reading storage",
        InfoOp::Stat => "Inspecting",
        InfoOp::PublicLink => "Creating link",
    };
    match target {
        Some(t) => format!("{verb} {t}\u{2026}"),
        None => format!("{verb}\u{2026}"),
    }
}
