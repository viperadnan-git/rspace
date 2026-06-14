//! Verify a single `sync/copy` (parent + include filter) copies both a file and
//! a directory, letting rclone resolve the type. Ignored by default.

use rspace_rclone_rc::{detect, Daemon};
use serde_json::{json, Value};

#[tokio::test]
#[ignore = "requires rclone"]
async fn unified_copy_handles_file_and_dir() {
    let rclone = detect().expect("rclone");
    let tmp = tempfile::tempdir().unwrap();
    let daemon = Daemon::start(&rclone.path, tmp.path().join("rcd.pid")).await.expect("daemon");
    let client = daemon.client();

    // Source tree: a file and a subdir-with-file.
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("subdir")).unwrap();
    std::fs::write(src.join("file.txt"), b"hello").unwrap();
    std::fs::write(src.join("subdir").join("inner.txt"), b"world").unwrap();

    let copy = |item: &str, dst: String| {
        let params = json!({
            "srcFs": src.to_string_lossy(),
            "dstFs": dst,
            "_filter": { "IncludeRule": [format!("/{item}"), format!("/{item}/**")] },
        });
        async move { client.call::<Value>("sync/copy", &params).await }
    };

    // Download the file.
    let d1 = tmp.path().join("dl1");
    copy("file.txt", d1.to_string_lossy().into_owned()).await.expect("copy file");
    println!("file -> {}", d1.join("file.txt").exists());
    assert!(d1.join("file.txt").exists(), "file not copied to dst/file.txt");

    // Download the directory (name preserved).
    let d2 = tmp.path().join("dl2");
    copy("subdir", d2.to_string_lossy().into_owned()).await.expect("copy dir");
    println!("dir -> {}", d2.join("subdir").join("inner.txt").exists());
    assert!(d2.join("subdir").join("inner.txt").exists(), "dir not copied to dst/subdir/");

    daemon.shutdown().await;
}
