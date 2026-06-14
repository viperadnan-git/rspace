//! Verify the literal-path transfer ops handle files (incl. glob-metachar names
//! like `[...]`), directories, and moves. Ignored by default.

use std::time::Duration;

use rspace_rclone_rc::{detect, Daemon, RcClient};
use serde_json::{json, Value};

async fn run_job(client: &RcClient, method: &str, params: Value) {
    let group = "rspace-test/0";
    let jobid =
        client.call_async(method, params, group).await.unwrap_or_else(|e| panic!("submit {method}: {e}"));
    for _ in 0..50 {
        let st = client.job_status(jobid).await.expect("job/status");
        if st.finished {
            assert!(st.success, "{method} failed: {}", st.error);
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{method} never finished");
}

#[tokio::test]
#[ignore = "requires rclone"]
async fn copyfile_handles_glob_metachar_name() {
    let rclone = detect().expect("rclone");
    let tmp = tempfile::tempdir().unwrap();
    let daemon = Daemon::start(&rclone.path, tmp.path().join("rcd.pid")).await.expect("daemon");
    let client = daemon.client();

    // A filename with brackets — a glob character class if treated as a pattern.
    let name = "King - [The Shining 02].epub";
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join(name), b"hi").unwrap();
    let dst = tmp.path().join("dst");

    run_job(
        client,
        "operations/copyfile",
        json!({
            "srcFs": src.to_string_lossy(), "srcRemote": name,
            "dstFs": dst.to_string_lossy(), "dstRemote": name,
        }),
    )
    .await;
    assert!(dst.join(name).exists(), "bracketed file not copied");

    daemon.shutdown().await;
}

#[tokio::test]
#[ignore = "requires rclone"]
async fn sync_copy_handles_dir_and_move_relocates() {
    let rclone = detect().expect("rclone");
    let tmp = tempfile::tempdir().unwrap();
    let daemon = Daemon::start(&rclone.path, tmp.path().join("rcd.pid")).await.expect("daemon");
    let client = daemon.client();

    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("dir")).unwrap();
    std::fs::write(src.join("dir").join("inner.txt"), b"x").unwrap();
    std::fs::write(src.join("file.txt"), b"y").unwrap();

    // Directory copy: dst fs is the named destination dir.
    let dcopy = tmp.path().join("dcopy");
    run_job(
        client,
        "sync/copy",
        json!({ "srcFs": src.join("dir").to_string_lossy(), "dstFs": dcopy.join("dir").to_string_lossy() }),
    )
    .await;
    assert!(dcopy.join("dir").join("inner.txt").exists(), "dir not copied");

    // File move: relocates and removes the source.
    let dmove = tmp.path().join("dmove");
    run_job(
        client,
        "operations/movefile",
        json!({
            "srcFs": src.to_string_lossy(), "srcRemote": "file.txt",
            "dstFs": dmove.to_string_lossy(), "dstRemote": "file.txt",
        }),
    )
    .await;
    assert!(dmove.join("file.txt").exists(), "file not moved to dst");
    assert!(!src.join("file.txt").exists(), "source not removed by move");

    daemon.shutdown().await;
}
