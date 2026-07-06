# Changelog

All notable changes to rspace are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[SemVer](https://semver.org/).

## [0.1.0] - Unreleased

First public release — a native desktop app for browsing and managing cloud
storage through rclone (macOS; Linux/Windows experimental).

### Added
- File browser over rclone: remote list, directory navigation, breadcrumbs,
  resizable panes, image/text/info preview.
- Keyboard-first navigation (arrows + vim-style) with multi-select and a
  command palette.
- Write ops: upload, copy, move, delete, mkdir, rename — including cross-remote
  copy/move and drag-and-drop.
- Transfer queue with live progress, error surfacing, and retry.
- Schema-driven remote management (add/edit/remove over RC, incl. OAuth).
- Guided rclone setup screen with manual-path fallback.
- In-app self-update via axoupdater.
- **Uninstall rspace** menu action: wipes all app data and trashes the app;
  leaves the rclone config and cloud files untouched.
