# rspace — Roadmap

A fast, keyboard-first app for your cloud storage: browse every remote like a
file manager and mount it with **zero extra installs**, using rclone's built-in NFS server and
the OS's native NFS client (no macFUSE/WinFsp kernel extension). Native
sync-provider APIs (File Provider / CFAPI) are a backlog aspiration — they need
paid developer programs.

## Status (2026-06-18)
Phase 1 is complete (the command palette and remote-management UI have now
shipped) and Phase 3's browser-side write ops shipped early — the browser is a
full read/write, keyboard-first file manager. The app bundle is **ad-hoc signed**
with entitlements + version (runs locally; users clear the quarantine flag).
**Not yet started:** the mount feature (Phase 2, now no-install NFS) and the
Settings storage & config page.

The **File Provider mount** that was the planned headline is **backlogged**: it
needs restricted entitlements (File Provider + App Group) authorized by a
provisioning profile — which requires a paid Apple Developer account — plus
notarization to run anywhere but the dev's machine. De-quarantine does *not*
unlock it (that's only Gatekeeper's launch check; entitlements are validated by a
separate layer). The mount feature is re-based on rclone's **built-in NFS server**
+ the OS's native NFS client, which needs no kernel extension and no install.

Deferred deliberately: the **teardown manifest** and **storage-accounting** APIs
were scaffolded then removed — speculative with no caller. They return with the
mount feature (the VFS cache is the first sizable on-disk artifact) and the
Settings page. Cache/blobs dirs were dropped too; only `config`/`logs`/`state`
exist now (mount adds a VFS cache dir).

Shipped beyond the original plan: file preview pane (image/text/info over
`--rc-serve`), drag-and-drop move/copy plus OS→app file-drop upload,
multi-select, per-provider icons, pinned remotes, resizable + persisted layout
(sidebar/preview/columns), transfer retry, an operation-registry command
palette, schema-driven add/edit/remove remotes (incl. OAuth), SQLite-backed
state + job history, promise-style notifications, a restartable rcd daemon with
status-bar controls, and a brand/home landing view.

## Principles
- **Speed is non-negotiable.** No design that trades it away. Async I/O, no
  blocking the UI thread, stream rather than buffer.
- **Keyboard-first.** Every action reachable without the mouse; discoverable via
  a command palette.
- **Single source of truth on disk.** All config, cache, blobs, logs live under
  one app root. Anything created *outside* it (OS-managed) is recorded in a
  teardown manifest so uninstall is total.
- **Root fixes only.** No band-aids; if a fix needs a refactor, propose first.

## Locked decisions
| Area | Decision |
|------|----------|
| rclone link | Long-lived `rclone rcd` daemon, driven over its JSON RC API |
| First platform | macOS (depth-first) |
| MVP | Browser-first: detect → config → read-only browse + keyboard nav |
| Cleanup | One app-root + teardown manifest replayed on uninstall |
| UI | gpui (git, pinned `rev`), Zed-style; no third-party component lib; keyboard-first |
| rclone install | User's responsibility; we auto-detect, else redirect to install docs |

## Mount strategy per OS
Primary goal: **mount with no extra install.** rclone runs an internal NFS server
and the OS's built-in NFS client mounts it — no kernel extension.
- **macOS** — `rclone nfsmount` to `~/rspace/<remote>` (built-in NFS client).
  Needs `--vfs-cache-mode writes` for a writable mount. No macFUSE, no sudo.
- **Linux** — `rclone mount` to `~/rspace/<remote>` (libfuse is normally
  preinstalled; no sudo for a user mount). Unmount via `fusermount -uz`.
- **Windows** — `rclone mount remote: *` to take the next free drive letter (a
  clean Explorer drive). Requires **WinFsp** (the one platform needing an install).
- **Opt-in, power-user** — macFUSE / FUSE-T on macOS for `rclone mount` when the
  user already has them: better app compatibility than NFS.

Backlogged native paths (need paid developer programs): macOS **File Provider**,
Windows **CFAPI** sync-root provider.

## Settings — storage & config management
A user-facing view over the single-source-of-truth model, so users can audit and
reclaim space without uninstalling. Modeled on battle-tested patterns (browser
"Clear browsing data", macOS app-storage breakdown, Docker Desktop disk usage).

- **Footprint summary** — total on-disk size at the top, broken down by category:
  config, **mount VFS cache**, logs, daemon state. (A distinct **OS-managed**
  bucket returns only if the backlogged File Provider / CFAPI paths ship, since
  those materialize files outside the app root.)
- **Async, progressive sizing** — directory sizes computed off the UI thread and
  cached; never block on a large cache scan.
- **Per-category clear** — each category clearable independently, with a
  confirmation that states exactly what is and isn't affected (e.g. clearing
  cache never touches remotes/config). Destructive actions are explicit.
- **VFS cache clear** — purge the mount cache directly (it lives under the app
  root). If the backlogged File Provider / CFAPI paths ship, their OS-managed
  storage is evicted via the OS API (not `rm`) and recorded in the manifest.
- **Config inspection** — list config file path(s) and the rclone config
  location, size, last-modified; reveal in Finder/Explorer; view contents
  **read-only with secrets redacted** by default.
- **Manifest visibility** — surface the teardown manifest so users see every
  artifact created outside the app root and what an uninstall would remove.

## UI stack (gpui)
**Zed-like UI built directly on gpui.** The look and interaction model follow
Zed: dense, keyboard-first, theme-driven. We do NOT use `gpui-component` or any
third-party component library — if Zed doesn't use it, we don't. Components are
hand-built on gpui (our own `ui` crate), referencing Zed's own patterns.
- Depend on gpui + gpui_platform as **git dependencies pinned to one `rev`** of
  `zed-industries/zed` (single source checkout, reproducible via Cargo.lock).
  Not the crates.io release — it lags `main` and the API tracks `main`.
- Current rev `b077f41` builds standalone with no patches. `main` HEAD can be
  transiently broken for external consumers (e.g. forked `smol`), so bump the
  rev deliberately and re-verify, never float to `main`.
- gpui crate license is Apache-2.0 (distribution-safe; the Zed *app* is GPL, the
  *crate* is not). Re-verify the pinned rev's license before release.
- Reference: `gpui-book` for concepts; Zed's `crates/ui` for component patterns.

## Workspace layout (Cargo, idiomatic)
```
rspace/
  Cargo.toml            # workspace
  crates/
    app/                # binary: wires core + ui
    core/               # domain: remotes, file tree, storage, manifest, cache
    rclone_rc/          # typed RC client + daemon/mount (NFS) lifecycle + detection
    ui/                 # gpui views, keybindings, command palette
    platform_macos/     # (backlog) File Provider bridge — see Backlog
    platform_windows/   # (backlog) CFAPI sync-root provider — see Backlog
    platform_linux/     # (reserved) Linux-specific mount bits, if any
```

## Phases

### Phase 0 — Foundations
- [x] Cargo workspace scaffold + lint config (clippy, rustfmt).
- [x] `storage`: single app-root resolver per OS convention (App Support /
      LOCALAPPDATA / XDG); subdirs for config, logs, state.
- [ ] ~~Teardown manifest~~ — deferred to Phase 2 (no external artifacts yet).
- [ ] ~~Storage accounting API~~ — deferred to Phase 3 Settings page.
- [x] rclone detection: PATH + common install locations (Homebrew, etc.);
      if missing, redirect to rclone install docs.
- [x] rcd daemon lifecycle: spawn on loopback, random auth token, health check,
      graceful shutdown + signal cleanup; `--rc-serve` for object previews.
- [x] gpui app shell + keybinding system foundation.
- [x] macOS `.app` bundle: icon, version, ad-hoc signed with entitlements (runs
      locally; users clear quarantine). Developer ID + notarization need a paid
      Apple account → backlog.

### Phase 1 — Browser MVP (macOS)
- [x] RC client: list remotes (`config/dump`), list dir (`operations/list`),
      item metadata.
- [x] File-browser UI: remote list, directory navigation, breadcrumb, resizable
      columns, pinned remotes, per-provider icons.
- [x] First-class keyboard nav (arrows + vim-style); multi-select.
- [x] Command palette (operation registry; browser actions reachable by keyboard).
- [x] Read-only actions: preview pane (image/text/info), copy path, download.

### Phase 2 — Mount (no-install, rclone NFS)
- [ ] Mount lifecycle: spawn `rclone nfsmount` (macOS) / `rclone mount` (Linux)
      per remote with `--vfs-cache-mode writes`; health-check, unmount, cleanup —
      reuse the rcd daemon-management pattern (process owned by core, not detached).
- [ ] Mount config: remote → mountpoint mapping, persisted; a sane default
      mountpoint under the user's home; auto-unmount on quit.
- [ ] UI: mount/unmount controls + live mount state (sidebar + status bar).
- [ ] VFS cache under the app root; size surfaced (feeds the Settings page).
- [ ] **Spike:** NFS write quirks on macOS (Finder/Office edge cases — rclone
      issues #7503/#7973); choose cache mode + flags that work for common apps.
- [ ] Opt-in: detect macFUSE/FUSE-T/WinFsp and offer `rclone mount` for better
      app compatibility where the user already has them.

### Phase 3 — Write ops & polish (browser-side shipped early)
- [x] Upload / copy / move / delete / mkdir / rename via RC in the browser
      (incl. cross-remote copy/move, drag-and-drop). Through FP: pending Phase 2.
- [x] Transfer queue + progress (RC `core/stats`), error surfacing, retry.
- [x] Remote management UI (schema-driven add/edit/remove over RC, incl. OAuth).
- [ ] Settings — storage & config page: footprint breakdown, per-category clear
      (incl. API-driven OS-managed eviction), config inspection + manifest view.

### Phase 4 — Windows & Linux
- [ ] Verify the Phase 2 mount on Windows (`rclone mount remote: *` drive letter;
      needs WinFsp) and Linux (`rclone mount` on libfuse). Command paths are in
      place; untested on those platforms. CFAPI native provider → backlog.
- [ ] Per-platform packaging: MSIX (Windows), AppImage/deb (Linux).

### Phase 5 — Packaging & uninstall
- [ ] Distributable bundles: ad-hoc `.app` + de-quarantine note (macOS; notarized
      build is backlog), MSIX (Windows), AppImage/deb (Linux).
- [ ] Uninstaller: wipe app-root + replay teardown manifest.
- [ ] Per-platform full-cleanup verification.

## Identity
- Product name: `rspace` (lowercase).
- Bundle identifier: `com.viperadnan.rspace` (domain `viperadnan.com`).

## Backlog (blocked or deferred)
- **File Provider mount (macOS)** — native, on-demand placeholder materialization;
  was the planned headline. BLOCKED: restricted File Provider + App Group
  entitlements need a provisioning profile (paid Apple Developer account), and
  distribution needs notarization. The Swift extension + IPC spike live here until
  an account exists.
- **Windows CFAPI sync-root provider** — native placeholder hydration; same
  paid-developer / native-integration class as File Provider. Revisit together.
- **Developer ID signing + notarization** — needs the paid Apple account; until
  then the app ships ad-hoc-signed and users clear the quarantine flag.
- **Tabs / split panes** — browse multiple folders/remotes at once, in tabs or a
  split, à la Zed's `Pane`/`Item` model. Deferred UI enhancement; the
  focusable-pane refactor (explorer/sidebar/preview as entities) is the
  groundwork it would build on.

## Open questions (track, don't guess)
- NFS mount write reliability on macOS for common apps (cache mode + flags).
- VFS cache layout and eviction policy.
- RC API auth: loopback-only + per-session random token — confirm sufficient.

## Resolved
- **gpui**: git dep pinned to a `rev` + gpui-component; Apache-2.0 (see UI stack).
- **Config inspection**: read-only in-app with secrets redacted + reveal in
  Finder/Explorer; edits go through the structured remote UI (Phase 3), never
  raw-text editing — avoids corrupting the rclone config.
