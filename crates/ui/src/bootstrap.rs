//! App entry point: asset source, window bootstrap, global key bindings.

use super::*;

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
            "folder", "file", "copy", "check", "settings", "alert", "download",
            "upload", "folder_open", "pin", "chevron_up", "chevron_down", "scissors", "clipboard",
            "refresh", "activity", "trash", "x", "edit", "cloud", "hard_drive", "server", "database",
            "lock", "image", "drive", "dropbox", "gcs", "b2", "box", "mega", "swift",
            "yandex", "nextcloud", "protondrive", "icloud", "onedrive", "s3", "azureblob", "smb",
            "googlephotos", "internetarchive", "zoho", "seafile", "mailru", "sharefile", "memory",
            "cache", "compress", "chunker", "union", "alias", "hasher", "owncloud", "sidebar_right",
            "split", "git_compare", "swap", "plus", "rclone", "tasks", "github", "search",
            "corner_down_left", "keyboard", "new_folder", "external_link"
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

/// The native menu bar. Items dispatch the same actions as the keymap, so gpui
/// shows each shortcut automatically; an item is inert unless the focused view
/// handles its action (the workspace handles all of these).
fn app_menus() -> Vec<Menu> {
    vec![
        Menu::new("rspace").items([
            MenuItem::action("Check for Updates", CheckForUpdates),
            MenuItem::separator(),
            MenuItem::action("Settings", OpenSettings),
            MenuItem::action("Restart Daemon", RestartDaemon),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", gpui::SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Uninstall rspace", Uninstall),
            MenuItem::action("Quit rspace", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Tab", NewTab),
            MenuItem::action("New Folder", NewFolder),
            MenuItem::action("New File", NewFile),
            MenuItem::separator(),
            MenuItem::action("Add Remote\u{2026}", AddRemote),
            MenuItem::separator(),
            MenuItem::action("Close Tab", CloseTab),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Copy", CopyEntry),
            MenuItem::action("Cut", CutEntry),
            MenuItem::action("Paste", PasteEntry),
            MenuItem::separator(),
            MenuItem::action("Rename\u{2026}", Rename),
            MenuItem::action("Delete", DeleteEntry),
            MenuItem::separator(),
            MenuItem::action("Select All", SelectAll),
        ]),
        Menu::new("View").items([
            MenuItem::action("Reload", Reload),
            MenuItem::separator(),
            MenuItem::action("Toggle Preview", TogglePreview),
            MenuItem::action("Toggle Tasks Panel", ToggleTasks),
            MenuItem::action("Toggle Sync Panel", ToggleSync),
            MenuItem::action("Toggle Split", ToggleSplit),
            MenuItem::action("Toggle Search", ToggleSearch),
            MenuItem::separator(),
            MenuItem::action("Zoom In", ZoomIn),
            MenuItem::action("Zoom Out", ZoomOut),
            MenuItem::action("Reset Zoom", ZoomReset),
        ]),
        Menu::new("Go").items([
            MenuItem::action("Back", GoBack),
            MenuItem::action("Forward", GoForward),
            MenuItem::action("Enclosing Folder", GoUp),
            MenuItem::separator(),
            MenuItem::action("Next Tab", NextTab),
            MenuItem::action("Previous Tab", PrevTab),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
            MenuItem::action("Toggle Full Screen", ToggleFullscreen),
        ]),
        Menu::new("Help").items([
            MenuItem::action("Command Palette\u{2026}", TogglePalette),
            MenuItem::action("Keyboard Shortcuts", ShowKeybindings),
        ]),
    ]
}

pub fn run(startup: Startup) {
    application().with_assets(Assets).run(move |cx: &mut App| {
        keymap::bind(cx);
        text_input::bind_keys(cx);
        picker::bind_keys(cx);
        cx.set_menus(app_menus());
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

