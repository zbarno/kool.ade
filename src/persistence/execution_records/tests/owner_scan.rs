use super::super::{owner::unfinished_for_owner_at, retention::with_retention_lock_path};
use std::{fs, sync::mpsc, time::Duration};

#[test]
fn owner_recovery_waits_for_atomic_record_writes() {
    // Production boundary: recovery must not inspect a record directory while
    // an atomic writer has a temporary sibling file. This prevents a valid
    // worker health probe from failing during concurrent execution updates.
    // Two threads and a temporary state root exercise the shared OS lock only.
    let root = tempfile::tempdir().unwrap();
    let record = super::record();
    let task_directory = root.path().join("executions").join(&record.task_id);
    fs::create_dir_all(&task_directory).unwrap();
    let temporary = task_directory.join(format!(
        ".{}.json.{}-0.koolade.tmp",
        record.execution_id,
        std::process::id()
    ));
    fs::write(&temporary, serde_json::to_vec(&record).unwrap()).unwrap();
    let final_path = task_directory.join(format!("{}.json", record.execution_id));
    let lock_path = root.path().join("execution-retention.lock");
    let (started_tx, started_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let mut scan_thread = None;

    with_retention_lock_path(&lock_path, || {
        let scan_root = root.path().to_path_buf();
        let owner = record.owner_instance_id.clone();
        scan_thread = Some(std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            result_tx
                .send(unfinished_for_owner_at(&scan_root, &owner))
                .unwrap();
        }));
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("recovery thread should start");
        assert!(matches!(
            result_rx.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        fs::rename(&temporary, &final_path).unwrap();
        Ok(())
    })
    .unwrap();

    assert_eq!(
        result_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("recovery should finish after the atomic rename")
            .unwrap(),
        vec![record]
    );
    scan_thread.unwrap().join().unwrap();
}
