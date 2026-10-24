use super::*;
use std::fs;

fn record() -> RemotePrivateFileExecutionGrantV1 {
    RemotePrivateFileExecutionGrantV1 {
        schema_version: SCHEMA_VERSION,
        execution_id: "00000000-0000-4000-8000-000000000001".into(),
        project_id: "00000000-0000-4000-8000-000000000002".into(),
        task_id: "00000000-0000-4000-8000-000000000003".into(),
        worker_id: "00000000-0000-4000-8000-000000000004".into(),
        controller_id: "00000000-0000-4000-8000-000000000005".into(),
        request_sequence: 7,
        cancellation_generation: 3,
        grant_ids: vec!["00000000-0000-4000-8000-000000000006".into()],
        start_payload_sha256: format!("sha256:{}", "b".repeat(64)),
        stage_complete: false,
        start_request_sequence: None,
    }
}

// Boundary: durable recovery state survives process reconstruction and binds
// execution, worker, controller, task, request, and grant IDs. A struct-only
// test cannot prove the serialized file is path-checked and safely replaceable.
// No Docker/SSH/network; setup cost is negligible.
#[test]
fn execution_grant_links_round_trip_as_private_metadata_only_records() {
    let root = tempfile::tempdir().unwrap();
    let grant = record();
    save(root.path(), &grant).unwrap();

    assert_eq!(
        load_for_worker(root.path(), &grant.worker_id).unwrap(),
        [grant]
    );
    let content = fs::read_to_string(super::store::record_path(
        &super::store::worker_directory(root.path(), "00000000-0000-4000-8000-000000000004")
            .unwrap(),
        "00000000-0000-4000-8000-000000000001",
    ))
    .unwrap();
    assert!(!content.contains("contents"));
    assert!(!content.contains("secret"));
}

// Boundary: a second start cannot reuse one execution ID for different grant
// authority. A map-only test cannot detect an on-disk binding change.
#[test]
fn execution_id_cannot_be_rebound_to_a_different_grant() {
    let root = tempfile::tempdir().unwrap();
    let original = record();
    save(root.path(), &original).unwrap();
    let mut changed = original.clone();
    changed.grant_ids[0] = "00000000-0000-4000-8000-000000000007".into();

    assert!(save(root.path(), &changed).is_err());
    assert_eq!(
        load_for_worker(root.path(), &original.worker_id).unwrap(),
        [original]
    );
}

// Boundary: the exact start payload and sequence must be durable before the
// worker RPC so restart replay is byte-for-byte stable. A struct test cannot
// prove the staged/start markers persist monotonically. No external services.
#[test]
fn staged_start_identity_is_persisted_before_replay() {
    let root = tempfile::tempdir().unwrap();
    let original = record();
    save(root.path(), &original).unwrap();

    assert!(
        bind_start_request(root.path(), &original.worker_id, &original.execution_id, 9,).is_err()
    );
    mark_stage_complete(root.path(), &original.worker_id, &original.execution_id).unwrap();
    bind_start_request(root.path(), &original.worker_id, &original.execution_id, 9).unwrap();
    bind_start_request(root.path(), &original.worker_id, &original.execution_id, 9).unwrap();
    assert!(
        bind_start_request(root.path(), &original.worker_id, &original.execution_id, 10,).is_err()
    );

    let restored = load_for_worker(root.path(), &original.worker_id).unwrap();
    assert!(restored[0].stage_complete);
    assert_eq!(restored[0].start_request_sequence, Some(9));
}

// Boundary: the backend must fail closed if recovery state is redirected to a
// symlink. In-memory validation cannot prove filesystem path safety.
#[cfg(unix)]
#[test]
fn execution_grant_link_rejects_symlinked_record() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let value = record();
    let directory = super::store::worker_directory(root.path(), &value.worker_id).unwrap();
    let target = root.path().join("outside.json");
    fs::write(&target, serde_json::to_vec(&value).unwrap()).unwrap();
    symlink(
        target,
        super::store::record_path(&directory, &value.execution_id),
    )
    .unwrap();

    assert!(load_for_worker(root.path(), &value.worker_id).is_err());
}
