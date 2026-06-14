# rspace

A fast, keyboard-first GUI for rclone: browse remotes like a file manager and
mount them through native OS sync-provider APIs. See [ROADMAP.md](ROADMAP.md).

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
cargo run -p rspace-app      # launch the app
```

## Layout

Cargo workspace under `crates/`: `core` (storage, manifest, accounting),
`rclone_rc` (detection, daemon, RC client), `ui` (gpui shell), `app` (binary),
and `platform_{macos,windows,linux}` (mount integration, later phases).
