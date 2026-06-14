//! End-to-end check of the async job flow against a real rclone. Ignored by
//! default. Run with `--ignored --nocapture`.

use std::time::Duration;

use rspace_rclone_rc::{detect, Daemon};

#[tokio::test]
#[ignore = "requires rclone installed"]
async fn async_copy_job_reports_status_and_stats() {
    let rclone = detect().expect("rclone installed");
    let dir = tempfile::tempdir().unwrap();
    let daemon = Daemon::start(&rclone.path, dir.path().join("rcd.pid")).await.expect("daemon");
    let client = daemon.client();

    // A source file and a destination dir, both local.
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("hello.txt"), vec![0u8; 4096]).unwrap();
    let dst = dir.path().join("dst");

    let group = "rspace-test/0";
    let params = serde_json::json!({
        "srcFs": src.to_string_lossy(),
        "srcRemote": "hello.txt",
        "dstFs": dst.to_string_lossy(),
        "dstRemote": "hello.txt",
    });

    let jobid = client
        .call_async("operations/copyfile", params, group)
        .await
        .expect("submit async copyfile");
    println!("jobid = {jobid}");

    // Poll until finished.
    let mut finished = false;
    for _ in 0..50 {
        let status = client.job_status(jobid).await.expect("job/status");
        let stats = client.stats(group).await.expect("core/stats");
        println!(
            "finished={} success={} err={:?} bytes={}/{}",
            status.finished, status.success, status.error, stats.bytes, stats.total_bytes
        );
        if status.finished {
            assert!(status.success, "copy failed: {}", status.error);
            finished = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(finished, "job never finished");
    assert!(dst.join("hello.txt").exists(), "file not copied");

    daemon.shutdown().await;
}
