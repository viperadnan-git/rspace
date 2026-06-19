//! ⌘⇧P command palette. Two stages on the reusable [`Picker`]:
//!
//! - **Commands** — fuzzy-search actions (with keybinding) and file operations.
//! - **Operation arguments** — selecting an operation (Copy, Move, …) enters a
//!   staged flow that collects its [`Operation::args`] one at a time, with badges
//!   for the chosen command/arguments and live path completion for path args.
//!
//! Operations and their requirements come from the single registry
//! ([`Operation`]); execution goes through `Workspace::run_operation`.

use std::collections::HashMap;

use gpui::{Action, FocusHandle, WeakEntity, Window};
use rspace_rclone_rc::split_parent;

use super::*;
use crate::fuzzy::fuzzy_match;
use crate::picker::{Confirmed, Picker, PickerDelegate};
use crate::query::Query;

/// Job operations offered in the palette. Destructive ones (Delete, Clean Up)
/// confirm before running; several aren't in the context menu at all.
const OPERATIONS: &[Operation] = &[
    Operation::Copy,
    Operation::Move,
    Operation::Sync,
    Operation::Delete,
    Operation::Cleanup,
    Operation::Rmdir,
    Operation::Rmdirs,
    Operation::MakeDir,
    Operation::Rename,
    Operation::CopyUrl,
    Operation::SetTier,
];

/// Read-only info ops offered in the palette.
const INFO_OPS: &[InfoOp] = &[InfoOp::Size, InfoOp::About, InfoOp::Stat, InfoOp::PublicLink];

/// A parameterized task: a job operation or a read-only query. Both collect
/// [`Self::args`] the same way; only the final execution differs.
#[derive(Clone, Copy)]
enum Task {
    Job(Operation),
    Info(InfoOp),
}

impl Task {
    fn label(self) -> &'static str {
        match self {
            Task::Job(op) => op.label(),
            Task::Info(op) => op.label(),
        }
    }

    fn args(self) -> &'static [ArgSpec] {
        match self {
            Task::Job(op) => op.args(),
            Task::Info(op) => op.args(),
        }
    }
}

/// A selectable command: an action to dispatch, or a task to configure.
enum Item {
    Action { action: Box<dyn Action>, label: &'static str, keystroke: SharedString },
    Task(Task),
}

impl Item {
    fn label(&self) -> &'static str {
        match self {
            Item::Action { label, .. } => label,
            Item::Task(t) => t.label(),
        }
    }
}

/// A visible row. `Item` rows belong to the command stage; the rest to a path stage.
#[derive(Clone)]
enum Candidate {
    Item(usize),
    Remote { name: String, kind: String },
    /// Pinned "use this folder" row that selects `remote:dir` as the argument.
    UseFolder { remote: String, dir: String },
    Entry { remote: String, path: String, name: String, is_dir: bool },
    /// Confirm the typed free text as a [`ArgKind::Name`] argument.
    UseName(String),
    /// Pick a whole remote as a [`ArgKind::Remote`] argument (no folder descent).
    PickRemote { name: String, kind: String },
    /// Go-to (`@`/`/` prefixes): navigate the explorer to a remote or folder.
    GotoRemote { name: String, kind: String },
    GotoPath { remote: String, path: String, name: String },
}

struct Row {
    candidate: Candidate,
    positions: Vec<usize>,
}

enum Mode {
    Commands,
    /// Collecting `task`'s arguments; `collected` holds the resolved ones so far.
    Args { task: Task, collected: Vec<ArgValue> },
}

pub(crate) struct CommandPaletteDelegate {
    workspace: WeakEntity<Workspace>,
    service: Service,
    db: Db,
    remotes: Vec<RemoteInfo>,
    /// The remote currently open in the explorer, for `/`-prefixed path jumps.
    current_remote: Option<String>,
    items: Vec<Item>,
    /// Command label → usage rank (0 = most-used); from `db` at open. Used
    /// commands sort first.
    usage_rank: HashMap<String, usize>,
    mode: Mode,
    rows: Vec<Row>,
    selected: usize,
    previous_focus: FocusHandle,
    path_query: Query<(String, String), Vec<Entry>>,
    /// The `(remote, dir)` we last asked `path_query` to load — re-asking only on
    /// change keeps the per-render refilter from re-triggering fetches.
    loaded_key: Option<(String, String)>,
}

impl CommandPaletteDelegate {
    pub(crate) fn new(
        previous_focus: FocusHandle,
        workspace: WeakEntity<Workspace>,
        service: Service,
        db: Db,
        remotes: Vec<RemoteInfo>,
        current_remote: Option<String>,
        window: &mut Window,
    ) -> Self {
        let mut items: Vec<Item> = action_defs()
            .into_iter()
            .map(|(action, label)| {
                let keystroke = window
                    .highest_precedence_binding_for_action(&*action)
                    .map(|b| b.keystrokes().iter().map(ToString::to_string).collect::<Vec<_>>().join(" "))
                    .unwrap_or_default();
                Item::Action { action, label, keystroke: keystroke.into() }
            })
            .collect();
        items.extend(OPERATIONS.iter().map(|op| Item::Task(Task::Job(*op))));
        items.extend(INFO_OPS.iter().map(|op| Item::Task(Task::Info(*op))));
        items.sort_by_key(|i| i.label());
        let usage_rank = db.command_rank().into_iter().enumerate().map(|(i, c)| (c, i)).collect();
        Self {
            workspace,
            service,
            db,
            remotes,
            current_remote,
            items,
            usage_rank,
            mode: Mode::Commands,
            rows: Vec::new(),
            selected: 0,
            previous_focus,
            // Short-lived picker: list each directory once, no revalidation.
            path_query: Query::new(None),
            loaded_key: None,
        }
    }

    /// The argument spec for the current stage (Args mode only).
    fn current_arg(&self) -> Option<ArgSpec> {
        match &self.mode {
            Mode::Args { task, collected } => task.args().get(collected.len()).copied(),
            Mode::Commands => None,
        }
    }

    fn match_commands(&mut self, query: &str) {
        let mut scored: Vec<(i32, usize, Vec<usize>)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| fuzzy_match(query, item.label()).map(|m| (m.score, i, m.positions)))
            .collect();
        // Most-used first (rank 0 = top); within a tier, fuzzy score then label.
        // On an empty query, ranking is purely usage → label.
        let rank = |i: usize| self.usage_rank.get(self.items[i].label()).copied().unwrap_or(usize::MAX);
        scored.sort_by(|a, b| {
            if query.is_empty() {
                rank(a.1).cmp(&rank(b.1))
            } else {
                b.0.cmp(&a.0).then_with(|| rank(a.1).cmp(&rank(b.1)))
            }
            .then_with(|| self.items[a.1].label().cmp(self.items[b.1].label()))
        });
        self.rows =
            scored.into_iter().map(|(_, i, positions)| Row { candidate: Candidate::Item(i), positions }).collect();
    }

    /// Fuzzy-match the remotes (pinned-first order preserved by the stable sort),
    /// turning each hit into a candidate via `make`.
    fn remote_rows(&self, query: &str, make: impl Fn(&RemoteInfo) -> Candidate) -> Vec<Row> {
        let mut scored: Vec<(i32, Row)> = self
            .remotes
            .iter()
            .filter_map(|r| {
                fuzzy_match(query, &r.name).map(|m| (m.score, Row { candidate: make(r), positions: m.positions }))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, r)| r).collect()
    }

    /// Ensure `remote:dir` is the directory `path_query` is serving (load once
    /// per key; the per-render call is a no-op while the key is unchanged).
    fn ensure_listing(&mut self, remote: &str, dir: &str, cx: &mut Context<Picker<Self>>) {
        let key = (remote.to_string(), dir.to_string());
        if self.loaded_key.as_ref() != Some(&key) {
            self.loaded_key = Some(key.clone());
            let service = self.service.clone();
            self.path_query.load(
                key,
                cx,
                |p| &mut p.delegate.path_query,
                move |(r, d)| async move { service.list_dir(&r, &d).await },
            );
        }
    }

    /// Rows for the loaded directory's entries, fuzzy-filtered by `partial` and
    /// newest-first (RFC3339 sorts chronologically as text). Empty while loading.
    fn entry_rows(&self, partial: &str, want_files: bool, make: impl Fn(&Entry) -> Candidate) -> Vec<Row> {
        let Some(entries) = self.path_query.data() else {
            return Vec::new();
        };
        let mut matched: Vec<(&str, Row)> = entries
            .iter()
            .filter(|e| want_files || e.is_dir)
            .filter_map(|e| {
                fuzzy_match(partial, &e.name)
                    .map(|m| (e.mod_time.as_str(), Row { candidate: make(e), positions: m.positions }))
            })
            .collect();
        matched.sort_by(|a, b| b.0.cmp(a.0));
        matched.into_iter().map(|(_, r)| r).collect()
    }

    /// `@`-prefix: jump to a remote.
    fn match_goto_remote(&mut self, query: &str) {
        self.loaded_key = None;
        self.rows = self.remote_rows(query, |r| Candidate::GotoRemote { name: r.name.clone(), kind: r.kind.clone() });
    }

    /// `/`-prefix: jump to a folder in the open remote (folders only).
    fn match_goto_path(&mut self, query: &str, cx: &mut Context<Picker<Self>>) {
        self.rows.clear();
        let Some(remote) = self.current_remote.clone() else {
            return;
        };
        let (dir, partial) = split_parent(query);
        self.ensure_listing(&remote, &dir, cx);
        self.rows = self.entry_rows(&partial, false, |e| Candidate::GotoPath {
            remote: remote.clone(),
            path: e.path.clone(),
            name: e.name.clone(),
        });
    }

    /// Free-text stage: the typed value is the argument (no completion).
    fn match_name(&mut self, query: &str) {
        self.loaded_key = None;
        self.rows.clear();
        let name = query.trim();
        if !name.is_empty() {
            self.rows.push(Row { candidate: Candidate::UseName(name.to_string()), positions: Vec::new() });
        }
    }

    /// Remote-only stage (e.g. About): pick a whole remote, no folder descent.
    fn match_remote_arg(&mut self, query: &str) {
        self.loaded_key = None;
        self.rows = self.remote_rows(query, |r| Candidate::PickRemote { name: r.name.clone(), kind: r.kind.clone() });
    }

    fn match_path(&mut self, query: &str, kind: ArgKind, cx: &mut Context<Picker<Self>>) {
        self.rows.clear();
        match parse_path(query) {
            PathLoc::Remotes { prefix } => {
                self.loaded_key = None;
                self.rows = self
                    .remote_rows(&prefix, |r| Candidate::Remote { name: r.name.clone(), kind: r.kind.clone() });
            }
            PathLoc::Dir { remote, dir, partial } => {
                self.ensure_listing(&remote, &dir, cx);
                // Pinned: choose the directory currently shown.
                self.rows.push(Row {
                    candidate: Candidate::UseFolder { remote: remote.clone(), dir: dir.clone() },
                    positions: Vec::new(),
                });
                let want_files = matches!(kind, ArgKind::SourcePath);
                let entries = self.entry_rows(&partial, want_files, |e| Candidate::Entry {
                    remote: remote.clone(),
                    path: e.path.clone(),
                    name: e.name.clone(),
                    is_dir: e.is_dir,
                });
                self.rows.extend(entries);
            }
        }
    }

    /// Push a resolved argument; advance to the next stage or execute when done.
    fn push_arg(&mut self, value: ArgValue, window: &mut Window, cx: &mut Context<Picker<Self>>) -> Confirmed {
        let Mode::Args { task, collected } = &mut self.mode else {
            return Confirmed::Dismiss;
        };
        let task = *task;
        collected.push(value);
        if collected.len() < task.args().len() {
            self.loaded_key = None;
            return Confirmed::Continue;
        }
        let args = std::mem::take(collected);
        self.db.record_command(task.label());
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| match task {
                Task::Job(op) => ws.run_operation(op, args, cx),
                Task::Info(op) => ws.run_info_op(op, args, cx),
            });
        }
        window.focus(&self.previous_focus, cx);
        Confirmed::Dismiss
    }

    /// Navigate the explorer to `remote:path` and dismiss.
    fn goto(&self, remote: String, path: String, cx: &mut Context<Picker<Self>>) -> Confirmed {
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| ws.navigate(remote, path, None, cx));
        }
        Confirmed::Dismiss
    }
}

/// A 15px svg glyph for a palette row icon.
fn glyph(path: &'static str, color: u32) -> impl IntoElement {
    gpui::svg().path(path).size(rem(15.0)).flex_shrink_0().text_color(rgb(color))
}

/// A palette row's icon + label, shrinkable so the label truncates.
fn icon_row(icon: impl IntoElement, label: impl IntoElement) -> impl IntoElement {
    h_flex().flex_1().min_w(px(0.0)).gap_2().child(icon).child(label)
}

/// Palette-offered actions, derived from the single keymap source of truth so
/// labels and keystrokes never drift from what's bound.
fn action_defs() -> Vec<(Box<dyn Action>, &'static str)> {
    crate::keymap::commands()
        .into_iter()
        .filter(|c| c.in_palette)
        .map(|c| (c.action, c.label))
        .collect()
}

enum PathLoc {
    /// No remote chosen yet — `prefix` filters the remote list.
    Remotes { prefix: String },
    /// Listing `remote:dir`, filtering its entries by `partial`.
    Dir { remote: String, dir: String, partial: String },
}

/// Split a path query into its completion location. The segment after the last
/// `/` is the partial being typed; everything before is the listed directory.
fn parse_path(query: &str) -> PathLoc {
    match query.split_once(':') {
        None => PathLoc::Remotes { prefix: query.to_string() },
        Some((remote, rest)) => {
            let (dir, partial) = split_parent(rest);
            PathLoc::Dir { remote: remote.to_string(), dir, partial }
        }
    }
}

/// A badge shown in the input row for the chosen command / collected arguments.
fn badge(text: impl Into<SharedString>, accent: bool) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .px(px(6.0))
        .py(px(1.0))
        .rounded_md()
        .text_xs()
        .bg(rgba(if accent { ACCENT_SOFT } else { OVERLAY }))
        .text_color(rgb(if accent { ACCENT } else { FG }))
        .child(text.into())
}

/// Short leaf label for an argument badge.
fn arg_summary(value: &ArgValue) -> String {
    match value {
        ArgValue::Path { remote, path, .. } => {
            path.rsplit('/').find(|s| !s.is_empty()).map(|s| s.to_string()).unwrap_or_else(|| format!("{remote}:"))
        }
        ArgValue::Name(n) => n.clone(),
    }
}

impl PickerDelegate for CommandPaletteDelegate {
    fn placeholder(&self) -> SharedString {
        match self.current_arg() {
            Some(arg) => format!("{}…", arg.label).into(),
            None => "Run a command…".into(),
        }
    }

    fn match_count(&self) -> usize {
        self.rows.len()
    }

    fn selected_index(&self) -> usize {
        self.selected
    }

    fn set_selected_index(&mut self, ix: usize, _: &mut Context<Picker<Self>>) {
        self.selected = ix;
    }

    fn update_matches(&mut self, query: &str, _: &mut Window, cx: &mut Context<Picker<Self>>) {
        match self.current_arg() {
            Some(arg) if matches!(arg.kind, ArgKind::Name) => self.match_name(query),
            Some(arg) if matches!(arg.kind, ArgKind::Remote) => self.match_remote_arg(query),
            Some(arg) => self.match_path(query, arg.kind, cx),
            // Command stage: `@` jumps to a remote, `/` to a path, else commands.
            None => match query.strip_prefix('@') {
                Some(rest) => self.match_goto_remote(rest),
                None => match query.strip_prefix('/') {
                    Some(rest) => self.match_goto_path(rest, cx),
                    None => self.match_commands(query),
                },
            },
        }
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
    }

    fn render_prefix(&self, _: &mut Context<Picker<Self>>) -> Option<AnyElement> {
        let Mode::Args { task, collected } = &self.mode else {
            return None;
        };
        let mut row = h_flex().gap_1().child(badge(task.label(), true));
        for arg in collected {
            row = row.child(badge(arg_summary(arg), false));
        }
        Some(row.into_any_element())
    }

    fn render_match(&self, ix: usize, selected: bool, _: &mut Context<Picker<Self>>) -> AnyElement {
        let Some(row) = self.rows.get(ix) else {
            return div().into_any_element();
        };
        let item = picker_item(ix, selected);
        match &row.candidate {
            Candidate::Item(i) => {
                let cmd = &self.items[*i];
                let keystroke = match cmd {
                    Item::Action { keystroke, .. } if !keystroke.is_empty() => Some(keystroke.clone()),
                    _ => None,
                };
                item.child(highlighted_label(cmd.label(), &row.positions, FG, ACCENT))
                    .when_some(keystroke, |el, k| el.child(key_binding(k)))
                    .into_any_element()
            }
            Candidate::Remote { name, kind }
            | Candidate::PickRemote { name, kind }
            | Candidate::GotoRemote { name, kind } => item
                .child(icon_row(glyph(remote_icon(kind), FG_MUTED), highlighted_label(name, &row.positions, FG, ACCENT)))
                .into_any_element(),
            Candidate::Entry { name, is_dir, .. } => item
                .child(icon_row(file_icon(*is_dir), highlighted_label(name, &row.positions, FG, ACCENT)))
                .into_any_element(),
            Candidate::GotoPath { name, .. } => item
                .child(icon_row(file_icon(true), highlighted_label(name, &row.positions, FG, ACCENT)))
                .into_any_element(),
            Candidate::UseFolder { remote, dir } => item
                .child(icon_row(
                    glyph("icons/check.svg", ACCENT),
                    h_flex()
                        .gap_2()
                        .min_w(px(0.0))
                        .child(div().flex_shrink_0().text_color(rgb(FG)).child("Use this folder"))
                        .child(div().min_w(px(0.0)).truncate().text_xs().text_color(rgb(FG_SUBTLE)).child(format!("{remote}:{dir}"))),
                ))
                .into_any_element(),
            Candidate::UseName(name) => item
                .child(icon_row(
                    glyph("icons/check.svg", ACCENT),
                    div().flex_1().min_w(px(0.0)).truncate().text_color(rgb(FG)).child(format!("Use \u{201c}{name}\u{201d}")),
                ))
                .into_any_element(),
        }
    }

    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> Confirmed {
        let Some(candidate) = self.rows.get(ix).map(|r| r.candidate.clone()) else {
            return Confirmed::Dismiss;
        };
        match candidate {
            Candidate::Item(i) => match &self.items[i] {
                Item::Action { action, .. } => {
                    // Actions run now, so count them now.
                    self.db.record_command(self.items[i].label());
                    let action = action.boxed_clone();
                    window.focus(&self.previous_focus, cx);
                    window.dispatch_action(action, cx);
                    Confirmed::Dismiss
                }
                // Tasks are counted on execution (push_arg), not on selection, so
                // an abandoned arg flow doesn't inflate the usage ranking.
                Item::Task(task) => {
                    self.mode = Mode::Args { task: *task, collected: Vec::new() };
                    self.loaded_key = None;
                    Confirmed::Continue
                }
            },
            // Descend into a remote or folder by completing its path.
            Candidate::Remote { name, .. } => Confirmed::SetQuery(format!("{name}:")),
            // Remote-only arg (About): the whole remote is the value.
            Candidate::PickRemote { name, .. } => {
                self.push_arg(ArgValue::Path { remote: name, path: String::new(), is_dir: true }, window, cx)
            }
            Candidate::Entry { remote, path, is_dir: true, .. } => {
                Confirmed::SetQuery(format!("{remote}:{path}/"))
            }
            Candidate::Entry { remote, path, is_dir: false, .. } => {
                self.push_arg(ArgValue::Path { remote, path, is_dir: false }, window, cx)
            }
            Candidate::UseFolder { remote, dir } => {
                self.push_arg(ArgValue::Path { remote, path: dir, is_dir: true }, window, cx)
            }
            Candidate::UseName(name) => self.push_arg(ArgValue::Name(name), window, cx),
            // Navigate the explorer and close.
            Candidate::GotoRemote { name, .. } => self.goto(name, String::new(), cx),
            Candidate::GotoPath { remote, path, .. } => self.goto(remote, path, cx),
        }
    }

    fn is_loading(&self) -> bool {
        // A path stage / go-to is fetching a directory we don't have cached yet.
        self.path_query.data().is_none() && self.path_query.is_fetching()
    }

    fn back(&mut self, _: &mut Window, _: &mut Context<Picker<Self>>) -> bool {
        match &mut self.mode {
            Mode::Commands => false,
            Mode::Args { collected, .. } => {
                if collected.pop().is_none() {
                    self.mode = Mode::Commands;
                }
                self.loaded_key = None;
                true
            }
        }
    }
}
