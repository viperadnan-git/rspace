<p align="center">
  <img src="assets/icon.png" alt="rspace" width="128" height="128">
</p>

<h1 align="center">rspace</h1>

<p align="center">
  <a href="https://github.com/viperadnan-git/rspace/releases"><img alt="Release" src="https://img.shields.io/github/v/release/viperadnan-git/rspace?style=plastic"></a>
  <a href="https://github.com/viperadnan-git/rspace/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/viperadnan-git/rspace/total?style=plastic"></a>
  <a href="https://github.com/viperadnan-git/rspace/actions/workflows/package.yml"><img alt="Build" src="https://img.shields.io/github/actions/workflow/status/viperadnan-git/rspace/package.yml?style=plastic&label=build"></a>
  <img alt="Code size" src="https://img.shields.io/github/languages/code-size/viperadnan-git/rspace?style=plastic">
  <img alt="Platform" src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey?style=plastic">
  <img alt="Rust version" src="https://img.shields.io/badge/dynamic/toml?url=https://raw.githubusercontent.com/viperadnan-git/rspace/main/Cargo.toml&query=%24.workspace.package%5B%27rust-version%27%5D&style=plastic&label=rust&logo=rust&logoColor=white&color=DEA584&prefix=%E2%89%A5">
  <img alt="Powered by rclone" src="https://img.shields.io/badge/powered%20by-rclone-3B7DED?style=plastic&logo=rclone&logoColor=white">
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/License-GPL--3.0-blue?style=plastic"></a>
</p>

rspace is a fast, native desktop app for your cloud storage. Connect Drive, S3,
Dropbox, and 70+ more providers, then browse, move, and sync files across all of
them from one window — no command line required. Built on rclone, with a clean
native UI on top.

## Download

Every link points at the **latest release**:

| Platform | Download |
| --- | --- |
| macOS · Apple Silicon | [rspace-macos-aarch64.dmg](https://github.com/viperadnan-git/rspace/releases/latest/download/rspace-macos-aarch64.dmg) |
| macOS · Intel | [rspace-macos-x86_64.dmg](https://github.com/viperadnan-git/rspace/releases/latest/download/rspace-macos-x86_64.dmg) |
| Windows · Installer | [rspace-windows-x86_64-setup.exe](https://github.com/viperadnan-git/rspace/releases/latest/download/rspace-windows-x86_64-setup.exe) |
| Windows · MSI | [rspace-windows-x86_64.msi](https://github.com/viperadnan-git/rspace/releases/latest/download/rspace-windows-x86_64.msi) |
| Linux · AppImage | [rspace-linux-x86_64.AppImage](https://github.com/viperadnan-git/rspace/releases/latest/download/rspace-linux-x86_64.AppImage) |
| Linux · Debian/Ubuntu | [rspace-linux-x86_64.deb](https://github.com/viperadnan-git/rspace/releases/latest/download/rspace-linux-x86_64.deb) |

macOS: open the `.dmg` and drag rspace to Applications. Linux: `chmod +x` the AppImage and run it. All releases are at [Releases](https://github.com/viperadnan-git/rspace/releases).

The app is **not code-signed** (no paid developer certificates), so each OS shows
a one-time "unidentified developer" prompt:

- **macOS** — right-click the app → **Open** (or `xattr -dr com.apple.quarantine /Applications/rspace.app`).
- **Windows** — SmartScreen → **More info → Run anyway**.

To uninstall, use **rspace → Uninstall rspace** — it wipes all app data and moves
the app to the Trash. Your rclone config and cloud files are left untouched.

## Prerequisites

- **Rust** — current stable (edition 2024; `rustc` ≥ 1.85).
- **rclone** — install it yourself ([rclone.org/install](https://rclone.org/install/)).
  rspace auto-detects it and prompts to install if missing.
- **macOS build only** — full **Xcode** (not just Command Line Tools) plus the
  **Metal Toolchain**, required to compile gpui's shaders:
  ```sh
  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
  sudo xcodebuild -license accept
  xcodebuild -downloadComponent MetalToolchain
  ```

## Build & run

```sh
cargo build --workspace      # build everything
cargo test --workspace       # run tests
cargo run -p rspace          # launch the app
```

## Layout

Cargo workspace under `crates/`: `core` (storage, manifest, accounting),
`rclone_rc` (detection, daemon, RC client), `ui` (gpui shell), `app` (binary),
and `platform_{macos,windows,linux}` (mount integration, later phases).

## Releasing

Releases are automated by [release-plz](https://release-plz.dev): every push to
`main` keeps a **Release PR** open that bumps the shared version and updates
`CHANGELOG.md` from the Conventional Commit history. **Merge that PR** to ship —
release-plz tags `vX.Y.Z`, which triggers [cargo-packager](https://github.com/crabnebula-dev/cargo-packager)
(`.github/workflows/package.yml`) to build the macOS/Linux/Windows installers and
publish the GitHub Release. No local release commands.

One-time repo secrets:

- `RELEASE_PLZ_TOKEN` — a PAT (`contents` + `pull-requests` write) so release-plz's
  tag can trigger the packager workflow.
- `CARGO_PACKAGER_SIGN_PRIVATE_KEY` / `CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD` —
  the updater signing key + password (generated via `cargo packager signer generate`)
  so `package.yml` can sign the auto-update artifacts. The public half is embedded
  in `crates/ui/src/update.rs`; in-app self-update verifies against it.

## License

[GPL-3.0-or-later](LICENSE). rspace is free software — you may use, study,
share, and modify it, but any distributed derivative must also be released
under the GPL; it cannot be taken closed-source. (gpui, which rspace builds on,
is Apache-2.0 — compatible with the GPL.)
