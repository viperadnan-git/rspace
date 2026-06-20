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
            "plus", "rclone", "tasks", "github", "search", "corner_down_left",
            "keyboard", "new_folder", "home"
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
        keymap::bind(cx);
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

