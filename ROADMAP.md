# rspace — Roadmap

A fast, keyboard-first GUI for rclone: browse remotes like a file manager and
mount them through native OS sync-provider APIs (no FUSE hacks where the OS
offers a real API).

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
- **macOS** — File Provider API (replicated extension).
- **Windows** — Cloud Files API (CFAPI) sync-root provider.
- **Linux** — rclone FUSE mount (no native sync-provider API exists).
- **Any OS, opt-in** — explicit rclone FUSE mount when the user asks.

## Settings — storage & config management
A user-facing view over the single-source-of-truth model, so users can audit and
reclaim space without uninstalling. Modeled on battle-tested patterns (browser
"Clear browsing data", macOS app-storage breakdown, Docker Desktop disk usage).

- **Footprint summary** — total on-disk size at the top, broken down by category:
  config, content cache / blobs, logs, daemon state, and **OS-managed** (File
  Provider materialized files / CFAPI placeholders) shown as a distinct bucket
  since it lives outside the app root.
- **Async, progressive sizing** — directory sizes computed off the UI thread and
  cached; never block on a large cache scan.
- **Per-category clear** — each category clearable independently, with a
  confirmation that states exactly what is and isn't affected (e.g. clearing
  cache never touches remotes/config). Destructive actions are explicit.
- **OS-managed eviction via API, not `rm`** — clearing File Provider/CFAPI storage
  goes through the OS API (evict items / domain APIs), recorded against the
  teardown manifest. Direct deletion is never used for OS-managed buckets.
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
    rclone_rc/          # typed RC API client + daemon lifecycle + path detection
    ui/                 # gpui views, keybindings, command palette
    platform_macos/     # File Provider bridge (Swift extension + FFI)
    platform_windows/   # CFAPI sync-root provider
    platform_linux/     # rclone mount management
```

## Phases

### Phase 0 — Foundations
- [ ] Cargo workspace scaffold + lint config (clippy, rustfmt).
- [ ] `storage`: single app-root resolver per OS convention (App Support /
      LOCALAPPDATA / XDG); subdirs for config, cache, blobs, logs.
- [ ] Teardown manifest: append-only record of external artifacts + replay API.
- [ ] Storage accounting API: per-category sizes (config/cache/blobs/logs/state +
      OS-managed), computed async and cached — backs the Settings view.
- [ ] rclone detection: PATH + common install locations (Homebrew, etc.);
      if missing, redirect to rclone install docs.
- [ ] rcd daemon lifecycle: spawn on loopback, random auth token, health check,
      graceful shutdown.
- [ ] gpui app shell + keybinding system foundation.
- [ ] macOS code-signing / dev-cert setup (needed by Phase 2; stand up early).

### Phase 1 — Browser MVP (macOS)
- [ ] RC client: list remotes (`config/dump`), list dir (`operations/list`),
      item metadata.
- [ ] File-browser UI: remote list, directory navigation, breadcrumb, columns.
- [ ] First-class keyboard nav (arrows + vim-style) and command palette.
- [ ] Read-only actions: preview/open, copy path, single-file download.

### Phase 2 — File Provider mount (macOS)
- [ ] File Provider replicated extension (Swift) bundled in the app.
- [ ] Domain register/unregister via `NSFileProviderManager` → write to manifest.
- [ ] Extension ↔ core bridge — **spike:** IPC vs direct RC API from a sandboxed
      extension; decide and document.
- [ ] Item enumeration + on-demand content fetch through rclone.
- [ ] **Spike:** verify exactly what FP persists and prove full removal.

### Phase 3 — Write ops & polish
- [ ] Upload / copy / move / delete / mkdir via RC, in browser and through FP.
- [ ] Transfer queue + progress (RC `core/stats`), robust error surfacing.
- [ ] Remote management UI (add/edit/remove via rclone config over RC).
- [ ] Settings — storage & config page: footprint breakdown, per-category clear
      (incl. API-driven OS-managed eviction), config inspection + manifest view.

### Phase 4 — Windows (CFAPI)
- [ ] CFAPI sync-root provider + placeholder hydration.
- [ ] Sync-root register/teardown → manifest.

### Phase 5 — Linux + explicit mount
- [ ] rclone FUSE mount lifecycle (Linux + opt-in elsewhere): mount, status,
      unmount, cleanup.

### Phase 6 — Packaging & uninstall
- [ ] Signed/notarized `.app` (macOS), MSIX (Windows), AppImage/deb (Linux).
- [ ] Uninstaller: wipe app-root + replay teardown manifest.
- [ ] Per-platform full-cleanup verification.

## Identity
- Product name: `rspace` (lowercase).
- Bundle identifier: `com.viperadnan.rspace` (domain `viperadnan.com`).

## Open questions (track, don't guess)
- FP extension ↔ core IPC mechanism (Phase 2 spike).
- Cache/blob store layout and eviction policy.
- RC API auth: loopback-only + per-session random token — confirm sufficient.

## Resolved
- **gpui**: git dep pinned to a `rev` + gpui-component; Apache-2.0 (see UI stack).
- **Config inspection**: read-only in-app with secrets redacted + reveal in
  Finder/Explorer; edits go through the structured remote UI (Phase 3), never
  raw-text editing — avoids corrupting the rclone config.
