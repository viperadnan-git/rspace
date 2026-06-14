//! End-to-end check against a real rclone install. Ignored by default since it
//! needs rclone on the system; run with `--ignored`.

use rspace_rclone_rc::{detect, Daemon};

#[tokio::test]
#[ignore = "requires rclone installed"]
async fn daemon_starts_lists_and_cleans_up() {
    let rclone = detect().expect("rclone installed");

    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("rcd.pid");

    let daemon = Daemon::start(&rclone.path, pidfile.clone()).await.expect("daemon starts");
    assert!(pidfile.exists(), "pid file written on start");

    // Health + a real RC call over the loopback API.
    let remotes = daemon.client().list_remotes().await.expect("list remotes");
    println!("remotes: {remotes:?}");

    daemon.shutdown().await;
    assert!(!pidfile.exists(), "pid file removed on graceful shutdown");
}
