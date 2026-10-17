use super::*;

#[test]
fn receipt_records_are_append_only_and_task_source_pinned() {
    let fixture = Fixture::new();
    let runtime = ProjectRuntimeStore::new(&fixture.store);
    let receipt_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let receipt = RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: receipt_id.clone(),
        operation: RuntimeOperation::Build,
        task_id: "task-001".into(),
        target_id: "web".into(),
        source_sha: "b".repeat(40),
        recipe_id: Some("build-web".into()),
        state: RuntimeReceiptState::Passed,
        exit_code: Some(0),
        artifact_hashes: Default::default(),
        log_reference: Some("log-001".into()),
        observed_at_unix_ms: 1,
    };
    runtime.append_receipt(&receipt).unwrap();
    assert_eq!(
        runtime.load_receipt(&receipt_id).unwrap(),
        Some(receipt.clone())
    );
    let other_task = RuntimeReceiptV1 {
        receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        task_id: "task-002".into(),
        ..receipt.clone()
    };
    runtime.append_receipt(&other_task).unwrap();
    assert_eq!(
        runtime.receipts_for_task("task-001").unwrap(),
        vec![receipt.clone()]
    );
    assert!(runtime.append_receipt(&receipt).is_err());
}

#[cfg(unix)]
#[test]
fn runtime_store_rejects_symlinked_state_paths() {
    let fixture = Fixture::new();
    let external = fixture.root.join("outside");
    std::fs::create_dir_all(&external).unwrap();
    std::fs::create_dir_all(&fixture.store.root).unwrap();
    let state = fixture.store.root.join("state");
    std::os::unix::fs::symlink(&external, &state).unwrap();
    let runtime = ProjectRuntimeStore::new(&fixture.store);
    assert!(matches!(
        runtime.load_manifest(),
        Err(StoreError::InvalidPath(_))
    ));
}
