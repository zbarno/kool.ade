use super::*;

#[test]
fn merges_upload_and_download_metrics_without_sensitive_fields() {
    let home = tempfile::tempdir().unwrap();
    let id = uuid::Uuid::new_v4().hyphenated().to_string();
    update_at(home.path(), &id, &id, "worker-1", |r| {
        r.workspace_upload_bytes = Some(12);
        r.workspace_upload_ms = Some(34);
    })
    .unwrap();
    update_at(home.path(), &id, &id, "worker-1", |r| {
        r.result_download_bytes = Some(56);
        r.result_download_ms = Some(78);
    })
    .unwrap();
    update_at(home.path(), &id, &id, "worker-1", |record| {
        record.image_digest = Some(format!("sha256:{}", "a".repeat(64)));
        record.execution_wall_ms = Some(90);
        record.execution_cpu_millis = Some(12);
        record.execution_peak_memory_bytes = Some(4096);
        record.worker_start_rpc_ms = Some(21);
        record.failure_code = Some(crate::execution::errors::ExecutionErrorKind::ExecutionFailed);
    })
    .unwrap();
    update_at(home.path(), &id, &id, "worker-1", |record| {
        record.harness = Some("codex".into());
        record.requested_model = Some("gpt-5-codex".into());
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
        (Some(12), Some(34), Some(56), Some(78))
    );
    assert_eq!(record.execution_wall_ms, Some(90));
    assert_eq!(record.harness.as_deref(), Some("codex"));
    assert_eq!(record.requested_model.as_deref(), Some("gpt-5-codex"));
    assert_eq!(record.execution_cpu_millis, Some(12));
    assert_eq!(record.execution_peak_memory_bytes, Some(4096));
    assert_eq!(record.worker_start_rpc_ms, Some(21));
    assert_eq!(
        record.failure_code,
        Some(crate::execution::errors::ExecutionErrorKind::ExecutionFailed)
    );
    assert_eq!(
        record.image_digest.as_deref(),
        Some(format!("sha256:{}", "a".repeat(64)).as_str())
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
    update_at(home.path(), &id, &id, "worker-1", |_| {}).unwrap();
    assert!(update_at(home.path(), &id, &id, "worker-2", |_| {}).is_err());
    assert!(
        update_at(home.path(), &id, &id, "worker-1", |r| r
            .workspace_upload_bytes =
            Some(MAX_BYTES + 1))
        .is_err()
    );
    assert!(
        update_at(home.path(), &id, &id, "worker-1", |r| r
            .worker_start_rpc_ms =
            Some(MAX_MS + 1))
        .is_err()
    );
    assert!(
        update_at(home.path(), &id, &id, "worker-1", |record| {
            record.requested_model = Some("gpt-5?token=synthetic".into());
        })
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
        update_at(&upload_root, &upload_id, &upload_id, "worker-1", |r| {
            r.workspace_upload_bytes = Some(11);
        })
        .unwrap();
    });
    let download_id = id.clone();
    let download_root = root.clone();
    let download = std::thread::spawn(move || {
        update_at(
            &download_root,
            &download_id,
            &download_id,
            "worker-1",
            |r| {
                r.result_download_bytes = Some(22);
            },
        )
        .unwrap();
    });
    upload.join().unwrap();
    download.join().unwrap();
    let record = load_at(&root, &id).unwrap().unwrap();
    assert_eq!(record.workspace_upload_bytes, Some(11));
    assert_eq!(record.result_download_bytes, Some(22));
}

#[test]
fn latest_task_record_is_available_without_a_controller_execution_record() {
    let home = tempfile::tempdir().unwrap();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let execution_id = uuid::Uuid::new_v4().hyphenated().to_string();
    update_at(home.path(), &execution_id, &task_id, "worker-1", |record| {
        record.workspace_upload_bytes = Some(512);
    })
    .unwrap();
    let record = query::latest_for_task_at(home.path(), &task_id)
        .unwrap()
        .unwrap();
    assert_eq!(record.task_id.as_deref(), Some(task_id.as_str()));
    assert_eq!(record.execution_id, execution_id);
    assert_eq!(record.workspace_upload_bytes, Some(512));
}

#[test]
fn legacy_zero_placeholders_are_loaded_as_unrecorded_metrics() {
    let home = tempfile::tempdir().unwrap();
    let id = uuid::Uuid::new_v4().hyphenated().to_string();
    let directory = home.path().join("remote-execution-telemetry");
    std::fs::create_dir_all(&directory).unwrap();
    let legacy = serde_json::json!({
        "schema_version": 3,
        "execution_id": id,
        "task_id": null,
        "worker_id": "worker-1",
        "workspace_upload_bytes": 0,
        "workspace_upload_ms": 0,
        "result_download_bytes": 0,
        "result_download_ms": 0,
        "execution_wall_ms": null,
        "execution_cpu_millis": 37,
        "execution_peak_memory_bytes": 4096,
        "image_digest": null
    });
    std::fs::write(
        record_path(home.path(), &id),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();

    let record = load_at(home.path(), &id).unwrap().unwrap();
    assert_eq!(record.workspace_upload_bytes, None);
    assert_eq!(record.workspace_upload_ms, None);
    assert_eq!(record.result_download_bytes, None);
    assert_eq!(record.result_download_ms, None);
    assert_eq!(record.execution_cpu_millis, Some(37));
    assert_eq!(record.execution_peak_memory_bytes, Some(4096));
    assert_eq!(record.worker_start_rpc_ms, None);
    assert_eq!(record.failure_code, None);
}

#[test]
fn updating_legacy_record_preserves_its_previous_time_ordering_proxy() {
    let home = tempfile::tempdir().unwrap();
    let id = uuid::Uuid::new_v4().hyphenated().to_string();
    let directory = home.path().join("remote-execution-telemetry");
    std::fs::create_dir_all(&directory).unwrap();
    let legacy = serde_json::json!({
        "schema_version": 3,
        "execution_id": id,
        "task_id": null,
        "worker_id": "worker-1",
        "workspace_upload_bytes": 0,
        "workspace_upload_ms": 0,
        "result_download_bytes": 0,
        "result_download_ms": 0,
        "execution_wall_ms": null,
        "execution_cpu_millis": 0,
        "execution_peak_memory_bytes": 0,
        "image_digest": null
    });
    let path = record_path(home.path(), &id);
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let previous_modified = std::fs::metadata(&path)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    update_at(
        home.path(),
        &id,
        &uuid::Uuid::new_v4().hyphenated().to_string(),
        "worker-1",
        |_| {},
    )
    .unwrap();
    let record = load_at(home.path(), &id).unwrap().unwrap();
    assert_eq!(record.started_at_unix_ms, Some(previous_modified));
    assert_eq!(record.execution_cpu_millis, None);
    assert_eq!(record.execution_peak_memory_bytes, None);
    assert_eq!(record.worker_start_rpc_ms, None);
    assert_eq!(record.failure_code, None);
}

#[test]
fn latest_task_record_uses_execution_start_time_after_late_updates() {
    let home = tempfile::tempdir().unwrap();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let older_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let newer_id = uuid::Uuid::new_v4().hyphenated().to_string();
    update_at(home.path(), &older_id, &task_id, "worker-1", |record| {
        record.started_at_unix_ms = Some(100);
    })
    .unwrap();
    update_at(home.path(), &newer_id, &task_id, "worker-1", |record| {
        record.started_at_unix_ms = Some(200);
    })
    .unwrap();
    // A late update touches the old file after the newer execution began.
    update_at(home.path(), &older_id, &task_id, "worker-1", |record| {
        record.workspace_upload_bytes = Some(1);
    })
    .unwrap();

    let record = query::latest_for_task_at(home.path(), &task_id)
        .unwrap()
        .unwrap();
    assert_eq!(record.execution_id, newer_id);
}
