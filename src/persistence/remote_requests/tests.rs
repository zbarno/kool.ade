use super::*;
use crate::execution::remote::protocol::{EnvelopeKindV1, PROTOCOL_VERSION_V2};

fn envelope() -> crate::execution::remote::protocol::EnvelopeV1 {
    crate::execution::remote::protocol::EnvelopeV1 {
        version: PROTOCOL_VERSION_V2,
        request_id: "00000000-0000-4000-8000-000000000001".into(),
        execution_id: "00000000-0000-4000-8000-000000000002".into(),
        sequence: 1,
        kind: EnvelopeKindV1::Health,
        payload: serde_json::json!({"wire_version": PROTOCOL_VERSION_V2}),
    }
}

#[test]
fn pending_request_survives_restart_and_completion_is_retrievable_once() {
    let root = tempfile::tempdir().unwrap();
    let request = envelope();
    save_pending_at(root.path(), "worker-east", &request).unwrap();
    save_pending_at(root.path(), "worker-east", &request).unwrap();
    assert_eq!(pending_at(root.path(), "worker-east").unwrap().len(), 1);

    let response = crate::execution::remote::protocol::EnvelopeV1 {
        kind: EnvelopeKindV1::Acknowledge,
        payload: serde_json::json!({"accepted": true}),
        ..request.clone()
    };
    record_completion_at(root.path(), "worker-east", &request.request_id, &response).unwrap();
    assert!(pending_at(root.path(), "worker-east").unwrap().is_empty());
    assert_eq!(
        take_completion_at(root.path(), "worker-east", &request.request_id)
            .unwrap()
            .unwrap()
            .response,
        response
    );
    assert!(
        take_completion_at(root.path(), "worker-east", &request.request_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn private_file_chunks_cannot_be_saved_in_the_controller_retry_store() {
    let root = tempfile::tempdir().unwrap();
    let mut request = envelope();
    request.kind = EnvelopeKindV1::PrivateFileChunk;
    request.payload = serde_json::json!({"bytes": "synthetic-secret-fixture"});

    assert!(save_pending_at(root.path(), "worker-east", &request).is_err());
    assert!(!root.path().join("remote-requests").exists());
}

#[test]
fn private_file_chunks_already_in_retry_storage_cannot_be_loaded_for_replay() {
    let root = tempfile::tempdir().unwrap();
    let mut request = envelope();
    request.kind = EnvelopeKindV1::PrivateFileChunk;
    request.payload = serde_json::json!({"bytes": [115, 121, 110, 116, 104, 101, 116, 105, 99]});
    let directory = worker_directory(root.path(), "worker-east", true).unwrap();
    let record = PendingRemoteRequestV1 {
        schema_version: 1,
        worker_id: "worker-east".into(),
        created_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
        envelope: request.clone(),
    };
    std::fs::write(
        pending_path(&directory, &request.request_id),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();

    assert!(pending_at(root.path(), "worker-east").is_err());
}

#[test]
fn private_file_chunk_responses_cannot_be_saved_in_controller_completion_store() {
    let root = tempfile::tempdir().unwrap();
    let request = envelope();
    let mut response = request.clone();
    response.kind = EnvelopeKindV1::PrivateFileChunk;
    response.payload = serde_json::json!({"bytes": "synthetic-secret-fixture"});

    assert!(
        record_completion_at(root.path(), "worker-east", &request.request_id, &response).is_err()
    );
    assert!(!root.path().join("remote-requests").exists());
}

#[test]
fn private_file_chunk_completions_already_on_disk_cannot_be_returned() {
    let root = tempfile::tempdir().unwrap();
    let request = envelope();
    let mut response = request.clone();
    response.kind = EnvelopeKindV1::PrivateFileChunk;
    response.payload = serde_json::json!({"token":"synthetic-secret"});
    let directory = worker_directory(root.path(), "worker-east", true).unwrap();
    let record = CompletedRemoteRequestV1 {
        schema_version: 1,
        worker_id: "worker-east".into(),
        request_id: request.request_id.clone(),
        completed_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
        response,
    };
    let path = completed_path(&directory, &request.request_id);
    std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();

    assert!(take_completion_at(root.path(), "worker-east", &request.request_id).is_err());
}

#[test]
fn pending_request_ids_cannot_be_rebound_and_symlink_roots_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let request = envelope();
    save_pending_at(root.path(), "worker-east", &request).unwrap();
    let mut conflicting = request.clone();
    conflicting.payload = serde_json::json!({"wire_version": PROTOCOL_VERSION_V2 + 1});
    assert!(save_pending_at(root.path(), "worker-east", &conflicting).is_err());

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let symlink_root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        symlink(other.path(), symlink_root.path().join("remote-requests")).unwrap();
        assert!(pending_at(symlink_root.path(), "worker-west").is_err());
    }
}

#[test]
fn expired_pending_request_is_completed_as_incompatible_and_never_replayed() {
    let root = tempfile::tempdir().unwrap();
    let request = envelope();
    save_pending_at(root.path(), "worker-east", &request).unwrap();
    let path = worker_directory(root.path(), "worker-east", false)
        .unwrap()
        .join(format!("{}.pending", request.request_id));
    let mut record: PendingRemoteRequestV1 =
        serde_json::from_slice(&read_private(&path).unwrap()).unwrap();
    record.created_at_unix_ms = 1;
    crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec(&record).unwrap()).unwrap();

    assert!(pending_at(root.path(), "worker-east").unwrap().is_empty());
    let completion = take_completion_at(root.path(), "worker-east", &request.request_id)
        .unwrap()
        .expect("expired envelope must become a durable completion");
    assert_eq!(completion.response.kind, EnvelopeKindV1::Error);
    let error: crate::execution::errors::ExecutionError =
        serde_json::from_value(completion.response.payload).unwrap();
    assert_eq!(
        error.kind,
        crate::execution::errors::ExecutionErrorKind::WorkerIncompatible
    );
}
