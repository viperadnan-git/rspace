//! Verify `deletefile` removes a file and `purge` removes a directory, against a
//! real rclone. Ignored by default.

use rspace_rclone_rc::{detect, Daemon};
use serde_json::{json, Value};

#[tokio::test]
#[ignore = "requires rclone"]
async fn delete_removes_file_and_dir() {
    let rclone = detect().expect("rclone");
    let tmp = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::start(rclone.path.clone(), tmp.path().join("rcd.pid")).await.expect("daemon");
    let client = daemon.client();

    let root = tmp.path().join("root");
    std::fs::create_dir_all(root.join("dir")).unwrap();
    std::fs::write(root.join("file.txt"), b"hello").unwrap();
    std::fs::write(root.join("dir").join("inner.txt"), b"world").unwrap();

    let fs = root.to_string_lossy().into_owned();

    // File: operations/deletefile.
    client
        .call::<Value>("operations/deletefile", &json!({ "fs": fs, "remote": "file.txt" }))
        .await
        .expect("deletefile");
    assert!(!root.join("file.txt").exists(), "file not deleted");

    // Directory (with contents): operations/purge.
    client
        .call::<Value>("operations/purge", &json!({ "fs": fs, "remote": "dir" }))
        .await
        .expect("purge");
    assert!(!root.join("dir").exists(), "dir not purged");

    daemon.shutdown().await;
}
