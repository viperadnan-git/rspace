//! `operations/check` returns its diff arrays as a normal result even when files
//! differ (success: false), and `parse_check` maps them to states. Ignored by default.

use rspace_rclone_rc::sync::{parse_check, DiffState};
use rspace_rclone_rc::{detect, Daemon};
use serde_json::{json, Value};

#[tokio::test]
#[ignore = "requires rclone"]
async fn check_reports_diff_arrays() {
    let rclone = detect().expect("rclone");
    let tmp = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::start(rclone.path.clone(), tmp.path().join("rcd.pid")).await.expect("daemon");
    let client = daemon.client();

    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(src.join("same.txt"), b"a").unwrap();
    std::fs::write(dst.join("same.txt"), b"a").unwrap();
    std::fs::write(src.join("only_src.txt"), b"b").unwrap();
    std::fs::write(dst.join("only_dst.txt"), b"c").unwrap();
    std::fs::write(src.join("diff.txt"), b"x").unwrap();
    std::fs::write(dst.join("diff.txt"), b"y").unwrap();

    // The call must succeed (HTTP 200 + body) despite the differences.
    let v: Value = client
        .call(
            "operations/check",
            &json!({ "srcFs": src.to_string_lossy(), "dstFs": dst.to_string_lossy(), "match": true }),
        )
        .await
        .expect("operations/check returns a result even when files differ");

    let entries = parse_check(&v);
    let state = |name: &str| entries.iter().find(|e| e.path == name).map(|e| e.state);
    assert_eq!(state("same.txt"), Some(DiffState::Match));
    assert_eq!(state("diff.txt"), Some(DiffState::Differ));
    assert_eq!(state("only_src.txt"), Some(DiffState::SrcOnly));
    assert_eq!(state("only_dst.txt"), Some(DiffState::DstOnly));

    daemon.shutdown().await;
}
