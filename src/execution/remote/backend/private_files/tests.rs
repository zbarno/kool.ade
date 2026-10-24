use super::*;
use crate::execution::{
    contract::WorkspaceRef,
    remote::private_files::{
        PrivateDataClassV1, PrivateFileSpecV1, RemotePrivateFileGrantV1,
        UserConfirmedPrivateFileGrantV1,
    },
};
use sha2::Digest;

fn confirmed_test_grant(now: u64) -> UserConfirmedPrivateFileGrantV1 {
    UserConfirmedPrivateFileGrantV1::after_confirmation(
        RemotePrivateFileGrantV1 {
            grant_id: "00000000-0000-4000-8000-000000000151".into(),
            controller_id: "00000000-0000-4000-8000-000000000152".into(),
            operator_id: String::new(),
            worker_id: "00000000-0000-4000-8000-000000000153".into(),
            worker_host_key_fingerprint: "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                .into(),
            workspace: WorkspaceRef {
                task_id: "00000000-0000-4000-8000-000000000154".into(),
                project_id: "00000000-0000-4000-8000-000000000155".into(),
                source_sha: "a".repeat(40),
            },
            confirmed_at_unix_ms: 0,
            expires_at_unix_ms: now + 60_000,
            files: vec![PrivateFileSpecV1 {
                path: ".env.production".into(),
                size_bytes: 6,
                sha256: format!("{:x}", sha2::Sha256::digest(b"secret")),
                data_class: PrivateDataClassV1::CredentialFile,
            }],
        },
        "synthetic-operator".into(),
        now,
    )
    .unwrap()
}

#[test]
fn a_revoked_receipt_is_inactive_for_an_already_staged_execution() {
    // Production boundary: exercises the same receipt check the remote backend
    // runs before event polling and result integration. Persistence tests alone
    // do not prove the active-execution guard sees a newly written revocation.
    // Uses a temporary receipt store; no worker, Docker, SSH, or network.
    let root = tempfile::tempdir().unwrap();
    let now = crate::persistence::harness_settings::current_unix_ms();
    let grant = UserConfirmedPrivateFileGrantV1::after_confirmation(
        RemotePrivateFileGrantV1 {
            grant_id: "00000000-0000-4000-8000-000000000141".into(),
            controller_id: "00000000-0000-4000-8000-000000000142".into(),
            operator_id: String::new(),
            worker_id: "00000000-0000-4000-8000-000000000143".into(),
            worker_host_key_fingerprint: "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                .into(),
            workspace: WorkspaceRef {
                task_id: "00000000-0000-4000-8000-000000000144".into(),
                project_id: "00000000-0000-4000-8000-000000000145".into(),
                source_sha: "a".repeat(40),
            },
            confirmed_at_unix_ms: 0,
            expires_at_unix_ms: now + 60_000,
            files: vec![PrivateFileSpecV1 {
                path: ".env.production".into(),
                size_bytes: 6,
                sha256: format!("{:x}", sha2::Sha256::digest(b"secret")),
                data_class: PrivateDataClassV1::CredentialFile,
            }],
        },
        "synthetic-operator".into(),
        now,
    )
    .unwrap()
    .grant;
    crate::persistence::remote_private_file_grants::record_user_confirmation(
        root.path(),
        UserConfirmedPrivateFileGrantV1 {
            grant: grant.clone(),
        },
        &[".env.production".into()],
    )
    .unwrap();

    assert!(grants_are_active_at(
        root.path(),
        std::slice::from_ref(&grant),
        now + 1
    ));
    crate::persistence::remote_private_file_grants::revoke(
        root.path(),
        &grant.workspace.project_id,
        &grant.grant_id,
        now + 2,
        crate::persistence::remote_private_file_grants::PrivateFileGrantRevocationReasonV1::OperatorRevoked,
    )
    .unwrap();
    assert!(!grants_are_active_at(
        root.path(),
        std::slice::from_ref(&grant),
        now + 3
    ));
}

#[test]
fn persisted_execution_link_restores_revocation_guard_after_backend_recreation() {
    // Production boundary: reconstructs the backend's execution-to-grant map
    // from disk and applies the same authorization check used before result
    // polling/integration. Receipt tests cannot prove that the association
    // survives backend recreation. No Docker/SSH/network; setup cost is low.
    let root = tempfile::tempdir().unwrap();
    let now = crate::persistence::harness_settings::current_unix_ms();
    let grant = crate::persistence::remote_private_file_grants::record_user_confirmation(
        root.path(),
        confirmed_test_grant(now),
        &[".env.production".into()],
    )
    .unwrap();
    let record =
        crate::persistence::remote_private_file_executions::RemotePrivateFileExecutionGrantV1 {
            schema_version: 1,
            execution_id: "00000000-0000-4000-8000-000000000156".into(),
            project_id: grant.workspace.project_id.clone(),
            task_id: grant.workspace.task_id.clone(),
            worker_id: grant.worker_id.clone(),
            controller_id: grant.controller_id.clone(),
            request_sequence: 12,
            cancellation_generation: 4,
            grant_ids: vec![grant.grant_id.clone()],
            start_payload_sha256: format!("sha256:{}", "c".repeat(64)),
            stage_complete: true,
            start_request_sequence: Some(13),
        };
    crate::persistence::remote_private_file_executions::save(root.path(), &record).unwrap();

    let recovered = recover_execution_grants_at(
        root.path(),
        &grant.worker_id,
        &grant.controller_id,
        &grant.worker_host_key_fingerprint,
    )
    .unwrap();
    let active = recovered.get(&record.execution_id).unwrap();
    assert_eq!(active.task_id, grant.workspace.task_id);
    assert_eq!(active.request_sequence, record.request_sequence);
    assert_eq!(
        active.cancellation_generation,
        record.cancellation_generation
    );
    assert!(active.stage_complete);
    assert_eq!(active.start_request_sequence, Some(13));
    assert_eq!(active.grants.as_slice(), std::slice::from_ref(&grant));
    assert!(grants_are_active_at(root.path(), &active.grants, now + 1));

    crate::persistence::remote_private_file_grants::revoke(
        root.path(),
        &grant.workspace.project_id,
        &grant.grant_id,
        now + 2,
        crate::persistence::remote_private_file_grants::PrivateFileGrantRevocationReasonV1::OperatorRevoked,
    )
    .unwrap();
    let recovered = recover_execution_grants_at(
        root.path(),
        &grant.worker_id,
        &grant.controller_id,
        &grant.worker_host_key_fingerprint,
    )
    .unwrap();
    let active = recovered.get(&record.execution_id).unwrap();
    assert!(!grants_are_active_at(root.path(), &active.grants, now + 3));
    assert!(
        recover_execution_grants_at(
            root.path(),
            &grant.worker_id,
            "00000000-0000-4000-8000-000000000999",
            &grant.worker_host_key_fingerprint,
        )
        .is_err()
    );
}
