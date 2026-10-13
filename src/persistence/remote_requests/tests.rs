use super::*;
use crate::execution::remote::protocol::{EnvelopeKindV1, PROTOCOL_VERSION_V1};

fn envelope() -> crate::execution::remote::protocol::EnvelopeV1 {
    crate::execution::remote::protocol::EnvelopeV1 {
        version: PROTOCOL_VERSION_V1,
        request_id: "00000000-0000-4000-8000-000000000001".into(),
        execution_id: "00000000-0000-4000-8000-000000000002".into(),
        sequence: 1,
        kind: EnvelopeKindV1::Health,
        payload: serde_json::json!({"wire_version": PROTOCOL_VERSION_V1}),
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
fn pending_request_ids_cannot_be_rebound_and_symlink_roots_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let request = envelope();
    save_pending_at(root.path(), "worker-east", &request).unwrap();
    let mut conflicting = request.clone();
    conflicting.payload = serde_json::json!({"wire_version": 2});
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
