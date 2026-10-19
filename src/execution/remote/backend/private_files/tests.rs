use super::*;
use crate::execution::{
    contract::WorkspaceRef,
    remote::private_files::{
        PrivateDataClassV1, PrivateFileSpecV1, RemotePrivateFileGrantV1,
        UserConfirmedPrivateFileGrantV1,
    },
};
use sha2::Digest;

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
