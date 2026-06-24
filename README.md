# rspace

All your cloud storage in one fast, native app — every remote feels like local
files, mounted with zero extra installs. Powered by rclone.
See [ROADMAP.md](ROADMAP.md).

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

## License

[GPL-3.0-or-later](LICENSE). rspace is free software — you may use, study,
share, and modify it, but any distributed derivative must also be released
under the GPL; it cannot be taken closed-source. (gpui, which rspace builds on,
is Apache-2.0 — compatible with the GPL.)
