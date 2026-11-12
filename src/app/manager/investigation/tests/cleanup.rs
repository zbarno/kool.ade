use super::super::{cleanup_after_cancellation_ack, watch_manager_cancellation};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Production boundary: Manager cancellation writes its marker and its
/// acknowledgement cleanup removes the temporary investigation receipt
/// directory. Helper-only tests cannot prove these background workers compose.
/// Cancellation and acknowledgement are synthetic; no provider, Docker,
/// network, or SSH. Setup cost is low.
#[test]
fn acknowledged_manager_cancellation_removes_only_its_temporary_receipts() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory
        .path()
        .join("koolade-investigation-synthetic-task");
    std::fs::create_dir(&root).unwrap();
    let cancellation_id = "synthetic-cancellation";
    let cancelled = Arc::new(AtomicBool::new(true));
    let stop = Arc::new(AtomicBool::new(false));
    watch_manager_cancellation(
        cancelled,
        stop.clone(),
        root.clone(),
        cancellation_id.into(),
    );
    let marker = root.join(format!(".cancel-{cancellation_id}.flag"));
    let marker_deadline = Instant::now() + Duration::from_secs(1);
    while !marker.exists() && Instant::now() < marker_deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    stop.store(true, Ordering::Release);
    assert!(marker.exists(), "cancellation watcher writes its marker");
    let receipt = root.join("receipt.json");
    std::fs::write(&receipt, "synthetic receipt").unwrap();
    std::fs::write(
        root.join(format!(".cancel-{cancellation_id}.ack")),
        "acknowledged",
    )
    .unwrap();

    cleanup_after_cancellation_ack(root.clone(), cancellation_id.into());
    let deadline = Instant::now() + Duration::from_secs(2);
    while root.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }

    assert!(!root.exists(), "acknowledged receipt root is cleaned up");
    assert!(
        directory.path().exists(),
        "the parent temporary root is retained"
    );
}
