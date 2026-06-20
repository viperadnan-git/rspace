mod logging;

use anyhow::Result;
use rspace_core::{mount_root, Db, Paths, SettingsStore};
use rspace_rclone_rc::{detect, from_path, reap_mount_orphans, Daemon, Rclone, Service, INSTALL_URL};
use rspace_ui::{run, RcloneStatus, Startup};

fn resolve_rclone(store: &SettingsStore) -> Option<Rclone> {
    if let Some(path) = store.get().rclone_path.as_deref() {
        if let Some(found) = from_path(path) {
            return Some(found);
        }
        tracing::warn!(path, "configured rclone path is invalid; falling back to detection");
    }
    detect().ok()
}

fn main() -> Result<()> {
    let paths = Paths::resolve()?;
    paths.ensure()?;
    // Held until the process exits so the background log writer flushes.
    let _log_guard = logging::init(paths.logs_dir());
    tracing::info!(
        config = %paths.config_dir().display(),
        data = %paths.data_dir().display(),
        cache = %paths.cache_dir().display(),
        "storage ready"
    );

    let store = SettingsStore::load(paths.settings_path());
    let db = Db::open(&paths.db_path(), &paths.history_db_path());

    // Point every rclone invocation (rcd, mounts, detection) at a custom config
    // file via the env var rclone reads. Safe: startup is single-threaded, before
    // the runtime or UI spawn any threads.
    if let Some(config) = store.get().rclone_config_path.clone() {
        unsafe { std::env::set_var("RCLONE_CONFIG", config) };
    }

    let Some(found) = resolve_rclone(&store) else {
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
            let service = Service::from_daemon(runtime.handle().clone(), daemon, found.path.clone());
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
