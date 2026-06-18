//! rclone integration: binary detection, `rclone rcd` lifecycle, RC API client.

pub mod client;
pub mod daemon;
pub mod detect;
pub mod mount;
pub mod ops;
mod proc;
pub mod service;

pub use client::{
    ConfigPaths, ConfigStep, Entry, JobStatus, OptionExample, Provider, RcClient, RcError,
    RemoteInfo, RemoteOption, Stats,
};
pub use daemon::{reap_orphan, Daemon, DaemonError};
pub use mount::{reap_orphans as reap_mount_orphans, CacheMode, MountConfig};
pub use detect::{detect, NotFound, Rclone, INSTALL_URL};
pub use ops::{join, split_parent, ArgKind, ArgSpec, ArgValue, InfoOp, InfoResult, Operation};
pub use service::{Service, ServiceError, TransferMode};
