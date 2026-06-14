use anyhow::Result;
use rspace_core::{Paths, SettingsStore};
use rspace_rclone_rc::{detect, Daemon, Service, INSTALL_URL};
use rspace_ui::{run, RcloneStatus, Startup};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let paths = Paths::resolve()?;
    paths.ensure()?;
    tracing::info!(root = %paths.root().display(), "storage ready");

    let store = SettingsStore::load(paths.settings_path());

    let Ok(found) = detect() else {
        tracing::warn!("rclone not found; prompting install from {INSTALL_URL}");
        run(Startup {
            rclone: RcloneStatus::Missing { install_url: INSTALL_URL.to_string() },
            service: None,
            paths: paths.clone(),
            store,
        });
        return Ok(());
    };
    tracing::info!(path = %found.path.display(), version = %found.version, "rclone detected");

    // reqwest's reactor lives on this runtime; the UI dispatches RC calls to it.
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let pidfile = paths.state_dir().join("rcd.pid");

    match runtime.block_on(Daemon::start(&found.path, pidfile)) {
        Ok(daemon) => {
            let service = Service::new(runtime.handle().clone(), daemon.client().clone());
            run(Startup {
                rclone: RcloneStatus::Found {
                    path: found.path.display().to_string(),
                    version: found.version,
                },
                service: Some(service),
                paths: paths.clone(),
                store,
            });
            runtime.block_on(daemon.shutdown());
        }
        Err(e) => {
            tracing::error!(error = %e, "rcd failed to start");
            run(Startup {
                rclone: RcloneStatus::Error { message: e.to_string() },
                service: None,
                paths: paths.clone(),
                store,
            });
        }
    }
    Ok(())
}
