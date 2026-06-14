//! rclone integration: binary detection, `rclone rcd` lifecycle, RC API client.

pub mod client;
pub mod daemon;
pub mod detect;
pub mod service;

pub use client::{Entry, JobStatus, RcClient, RcError, RemoteInfo, Stats};
pub use daemon::{reap_orphan, Daemon, DaemonError};
pub use detect::{detect, NotFound, Rclone, INSTALL_URL};
pub use service::{Service, ServiceError};
