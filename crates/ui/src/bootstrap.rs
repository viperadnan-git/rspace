//! App entry point: asset source, window bootstrap, global key bindings.

use super::*;

// Component-owned actions, declared in their modules; bound centrally here.
use crate::confirm::ConfirmAccept;
use crate::explorer::{CloseSearch, SearchSubmit};
use crate::mount_options::{MountCancel, MountSave};
use crate::number_field::NumberCommit;
use crate::prompt::{PromptCancel, PromptSubmit};
use crate::remotes::{ConfigConfirm, ConfigNext, ConfigPrev, FocusNext, FocusPrev};
use crate::status_screen::SetupSubmit;

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        // Brand mark (transparent; tinted by svg()). app-icon.png/.icns derive
        // from it via scripts/make_icns.sh.
        if path == "logo.svg" {
            return Ok(Some(std::borrow::Cow::Borrowed(
                include_bytes!("../../app/resources/logo.svg").as_slice(),
            )));
        }
        macro_rules! icons {
            ($($name:literal),* $(,)?) => {
                match path {
                    $(concat!("icons/", $name, ".svg") => Some(std::borrow::Cow::Borrowed(
                        include_bytes!(concat!("../assets/icons/", $name, ".svg")).as_slice(),
                    )),)*
                    _ => None,
                }
            };
        }
        Ok(icons!(
            "folder", "file", "copy", "check", "settings", "alert", "maximize", "minimize", "download",
            "upload", "folder_open", "pin", "chevron_up", "chevron_down", "scissors", "clipboard",
            "refresh", "activity", "trash", "x", "edit", "cloud", "hard_drive", "server", "database",
            "lock", "image", "drive", "dropbox", "gcs", "b2", "box", "mega", "swift",
            "yandex", "nextcloud", "protondrive", "icloud", "onedrive", "s3", "azureblob", "smb",
            "googlephotos", "internetarchive", "zoho", "seafile", "mailru", "sharefile", "memory",
            "cache", "compress", "chunker", "union", "alias", "hasher", "owncloud", "sidebar_right",
            "plus", "server_network", "server_network_off", "github", "search", "corner_down_left"
        ))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

pub enum RcloneStatus {
    Found { path: String, version: String },
    Missing { install_url: String },
    Error { message: String },
}

/// Startup state. `service` is present only when the daemon started.
pub struct Startup {
    pub rclone: RcloneStatus,
    pub service: Option<Service>,
    pub paths: Paths,
    pub store: SettingsStore,
    pub db: Db,
}

pub fn run(startup: Startup) {
    application().with_assets(Assets).run(move |cx: &mut App| {
        bind_keys(cx);
        text_input::bind_keys(cx);
        picker::bind_keys(cx);
        cx.set_menus(vec![
            Menu::new("rspace").items([MenuItem::action("Quit rspace", Quit)]),
            Menu::new("Window").items([
                MenuItem::action("Minimize", Minimize),
                MenuItem::action("Zoom", Zoom),
                MenuItem::action("Toggle Full Screen", ToggleFullscreen),
            ]),
        ]);
        cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1000.0), px(640.0)), cx);
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: None,
                appears_transparent: true,
                traffic_light_position: Some(point(px(9.0), px(9.0))),
            }),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };

        let Startup { rclone, service, paths, store, db } = startup;
        match service {
            Some(service) => {
                let (rclone_bin, version) = match &rclone {
                    RcloneStatus::Found { path, version } => (path.clone(), version.clone()),
                    _ => (String::new(), String::new()),
                };
                cx.open_window(options, |window, cx| {
                    cx.new(|cx| Workspace::new(service, rclone_bin, version, paths, store, db, window, cx))
                })
                .unwrap();
            }
            None => {
                cx.open_window(options, |_, cx| cx.new(|cx| StatusScreen::new(rclone, store, cx)))
                    .unwrap();
            }
        }
        cx.activate(true);
    });
}

fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-q", Quit, None),
        // macOS native chord + the F11 convention used on Linux/Windows.
        KeyBinding::new("ctrl-cmd-f", ToggleFullscreen, None),
        KeyBinding::new("f11", ToggleFullscreen, None),
        // Navigation/mutation are inert while a confirm dialog owns the keyboard.
        KeyBinding::new("down", SelectNext, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("j", SelectNext, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("up", SelectPrev, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("k", SelectPrev, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("shift-down", SelectNext, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("shift-j", SelectNext, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("shift-up", SelectPrev, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("shift-k", SelectPrev, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-a", SelectAll, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("enter", Open, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("tab", TogglePane, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("backspace", GoUp, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-[", GoBack, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-]", GoForward, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-r", Reload, Some("Workspace && !modal && !TextInput")),
        // Toggle (not !modal) so it can also close itself; the handler ignores
        // it while another modal is open.
        // The modern "cmdk" command-menu shortcut: cmd-k on macOS, ctrl-k elsewhere.
        KeyBinding::new("secondary-k", TogglePalette, Some("Workspace")),
        KeyBinding::new("left", FocusSidebar, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("right", FocusExplorer, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-c", CopyEntry, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-x", CutEntry, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-v", PasteEntry, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-backspace", DeleteEntry, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-shift-n", NewFolder, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("secondary-u", NewFile, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("f2", Rename, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("space", TogglePreview, Some("Workspace && !modal && !TextInput")),
        KeyBinding::new("escape", CloseSettings, Some("Workspace")),
        // Add/edit-remote dialog: arrows (or ctrl-n/p) navigate the picker,
        // Enter advances. Bound to its own context so any focusable list can reuse.
        KeyBinding::new("down", ConfigNext, Some("RemoteConfig")),
        KeyBinding::new("ctrl-n", ConfigNext, Some("RemoteConfig")),
        KeyBinding::new("up", ConfigPrev, Some("RemoteConfig")),
        KeyBinding::new("ctrl-p", ConfigPrev, Some("RemoteConfig")),
        // Enter confirms the current step from anywhere in the modal (matching the
        // other dialogs), so blurring a field doesn't disable it.
        KeyBinding::new("enter", ConfigConfirm, Some("RemoteConfig")),
        KeyBinding::new("tab", FocusNext, Some("RemoteConfig")),
        KeyBinding::new("shift-tab", FocusPrev, Some("RemoteConfig")),
        KeyBinding::new("enter", ConfirmAccept, Some("Confirm")),
        KeyBinding::new("enter", PromptSubmit, Some("Prompt")),
        KeyBinding::new("escape", PromptCancel, Some("Prompt")),
        KeyBinding::new("enter", MountSave, Some("MountOptions")),
        KeyBinding::new("escape", MountCancel, Some("MountOptions")),
        KeyBinding::new("enter", SetupSubmit, Some("Setup")),
        KeyBinding::new("enter", NumberCommit, Some("NumberField")),
        KeyBinding::new("enter", SearchSubmit, Some("ExplorerSearch")),
        // Toggle works while the search field is focused too, so it can close it.
        KeyBinding::new("secondary-f", ToggleSearch, Some("Workspace && !modal")),
        KeyBinding::new("escape", CloseSearch, Some("ExplorerSearch")),
        KeyBinding::new("secondary-=", ZoomIn, Some("Workspace && !modal")),
        KeyBinding::new("secondary-+", ZoomIn, Some("Workspace && !modal")),
        KeyBinding::new("secondary--", ZoomOut, Some("Workspace && !modal")),
        KeyBinding::new("secondary-0", ZoomReset, Some("Workspace && !modal")),
    ]);
    // Minimize is a macOS app convention (cmd-m); elsewhere the window manager owns it.
    #[cfg(target_os = "macos")]
    cx.bind_keys([KeyBinding::new("cmd-m", Minimize, None)]);
}
