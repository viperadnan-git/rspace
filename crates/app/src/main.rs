mod logging;

use anyhow::Result;
use rspace_core::{mount_root, Db, Paths, SettingsStore};
use rspace_rclone_rc::{detect, reap_mount_orphans, Daemon, Service, INSTALL_URL};
use rspace_ui::{run, RcloneStatus, Startup};

fn main() -> Result<()> {
    let paths = Paths::resolve()?;
    paths.ensure()?;
    // Held until the process exits so the background log writer flushes.
    let _log_guard = logging::init(&paths.logs_dir());
    tracing::info!(root = %paths.root().display(), "storage ready");

    let store = SettingsStore::load(paths.settings_path());
    let db = Db::open(&paths.db_path(), &paths.history_db_path());

    let Ok(found) = detect() else {
        tracing::warn!("rclone not found; prompting install from {INSTALL_URL}");
        run(Startup {
            rclone: RcloneStatus::Missing { install_url: INSTALL_URL.to_string() },
            service: None,
            paths: paths.clone(),
            store,
            db: db.clone(),
        });
        return Ok(());
    };
    tracing::info!(path = %found.path.display(), version = %found.version, "rclone detected");

    // Clear any mount left by a crashed previous run before starting fresh.
    if let Some(root) = mount_root() {
        reap_mount_orphans(&root);
    }

    // reqwest's reactor lives on this runtime; the UI dispatches RC calls to it.
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let pidfile = paths.pid_path();

    match runtime.block_on(Daemon::start(found.path.clone(), pidfile)) {
        Ok(daemon) => {
            let service = Service::from_daemon(
                runtime.handle().clone(),
                daemon,
                found.path.clone(),
                paths.mount_cache_dir(),
            );
            service.install_signal_cleanup();
            run(Startup {
                rclone: RcloneStatus::Found {
                    path: found.path.display().to_string(),
                    version: found.version,
                },
                service: Some(service.clone()),
                paths: paths.clone(),
                store,
                db: db.clone(),
            });
            runtime.block_on(service.shutdown());
        }
        Err(e) => {
            tracing::error!(error = %e, "rcd failed to start");
            run(Startup {
                rclone: RcloneStatus::Error { message: e.to_string() },
                service: None,
                paths: paths.clone(),
                store,
                db: db.clone(),
            });
        }
    }
    Ok(())
}
