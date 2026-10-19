use super::*;

#[test]
fn merges_upload_and_download_metrics_without_sensitive_fields() {
    let home = tempfile::tempdir().unwrap();
    let id = uuid::Uuid::new_v4().hyphenated().to_string();
    update_at(home.path(), &id, "worker-1", |r| {
        r.workspace_upload_bytes = 12;
        r.workspace_upload_ms = 34;
    })
    .unwrap();
    update_at(home.path(), &id, "worker-1", |r| {
        r.result_download_bytes = 56;
        r.result_download_ms = 78;
    })
    .unwrap();
    let record = load_at(home.path(), &id).unwrap().unwrap();
    assert_eq!(
        (
            record.workspace_upload_bytes,
            record.workspace_upload_ms,
            record.result_download_bytes,
            record.result_download_ms
        ),
        (12, 34, 56, 78)
    );
    let text = fs::read_to_string(record_path(home.path(), &id)).unwrap();
    assert!(
        !text.contains("hostname") && !text.contains("credential") && !text.contains("command")
    );
}

#[test]
fn rejects_worker_rebinding_and_unbounded_metrics() {
    let home = tempfile::tempdir().unwrap();
    let id = uuid::Uuid::new_v4().hyphenated().to_string();
    update_at(home.path(), &id, "worker-1", |_| {}).unwrap();
    assert!(update_at(home.path(), &id, "worker-2", |_| {}).is_err());
    assert!(
        update_at(home.path(), &id, "worker-1", |r| r.workspace_upload_bytes =
            MAX_BYTES + 1)
        .is_err()
    );
}

#[test]
fn concurrent_updates_preserve_both_transfer_directions() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    let id = uuid::Uuid::new_v4().hyphenated().to_string();
    let upload_id = id.clone();
    let upload_root = root.clone();
    let upload = std::thread::spawn(move || {
        update_at(&upload_root, &upload_id, "worker-1", |r| {
            r.workspace_upload_bytes = 11;
        })
        .unwrap();
    });
    let download_id = id.clone();
    let download_root = root.clone();
    let download = std::thread::spawn(move || {
        update_at(&download_root, &download_id, "worker-1", |r| {
            r.result_download_bytes = 22;
        })
        .unwrap();
    });
    upload.join().unwrap();
    download.join().unwrap();
    let record = load_at(&root, &id).unwrap().unwrap();
    assert_eq!(record.workspace_upload_bytes, 11);
    assert_eq!(record.result_download_bytes, 22);
}
