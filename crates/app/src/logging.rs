//! Process logging: a non-blocking, daily-rotating file plus stderr, filtered by
//! `RUST_LOG` (default `info`). The background writer keeps I/O off the hot path.
//!
//! Verbose timing: `RUST_LOG=rspace_rclone_rc=debug` logs every RC call's
//! duration; `=trace` also includes the per-second job/stats polls.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

/// Returned guard flushes the background writer on drop; hold it for the whole
/// process lifetime.
pub fn init(logs_dir: &Path) -> WorkerGuard {
    let file = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("rspace")
        .filename_suffix("log")
        .max_log_files(7)
        .build(logs_dir)
        .expect("create log file appender");
    let (writer, guard) = tracing_appender::non_blocking(file);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_ansi(false).with_writer(writer))
        .with(fmt::layer().with_writer(std::io::stderr))
        .init();

    guard
}
