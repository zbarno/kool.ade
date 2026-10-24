use super::*;
use crate::execution::{
    contract::WorkspaceRef,
    remote::private_files::{
        PrivateDataClassV1, PrivateFileSpecV1, UserConfirmedPrivateFileGrantV1,
    },
};
use sha2::Digest;

fn confirmed_grant(now: u64) -> UserConfirmedPrivateFileGrantV1 {
    UserConfirmedPrivateFileGrantV1::after_confirmation(
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
        "synthetic-operator-141".into(),
        now,
    )
    .unwrap()
}

#[test]
fn file_backed_receipt_binds_exact_grant_and_revocation() {
    // Production boundary: exercises the same file-backed store and query used
    // by controller authorization, proving exact target/content binding and
    // revocation survive reload. A struct validator alone cannot prove atomic
    // persistence/reload. Uses a temporary directory, no network or SSH (low cost).
    let root = tempfile::tempdir().unwrap();
    let now = crate::persistence::harness_settings::current_unix_ms();
    let confirmed = confirmed_grant(now);
    let exclusions = vec![".env.production".to_owned()];
    let grant = record_user_confirmation(root.path(), confirmed.clone(), &exclusions).unwrap();
    assert_eq!(
        record_user_confirmation(root.path(), confirmed, &exclusions).unwrap(),
        grant
    );
    assert_eq!(
        require_authorized(root.path(), &grant, now + 1).unwrap(),
        grant
    );

    let contents =
        std::fs::read(store::path(root.path(), &grant.workspace.project_id).unwrap()).unwrap();
    assert!(!contents.windows(b"secret".len()).any(|w| w == b"secret"));

    let mut forged_hash = grant.clone();
    forged_hash.files[0].sha256 = "0".repeat(64);
    assert!(require_authorized(root.path(), &forged_hash, now + 1).is_err());
    let mut forged_worker = grant.clone();
    forged_worker.worker_id = "00000000-0000-4000-8000-000000000147".into();
    assert!(require_authorized(root.path(), &forged_worker, now + 1).is_err());
    assert!(record_user_confirmation(root.path(), confirmed_grant(now), &[]).is_err());

    let revoked = revoke(
        root.path(),
        &grant.workspace.project_id,
        &grant.grant_id,
        now + 2,
        PrivateFileGrantRevocationReasonV1::OperatorRevoked,
    )
    .unwrap();
    assert_eq!(
        revoke(
            root.path(),
            &grant.workspace.project_id,
            &grant.grant_id,
            now + 3,
            PrivateFileGrantRevocationReasonV1::WorkerIdentityChanged,
        )
        .unwrap(),
        revoked,
        "repeat revocation retains the first immutable receipt"
    );
    assert!(require_authorized(root.path(), &grant, now + 4).is_err());
}

#[test]
fn private_grant_receipt_rejects_expiry_and_cross_project_binding() {
    let root = tempfile::tempdir().unwrap();
    let now = crate::persistence::harness_settings::current_unix_ms();
    let grant = record_user_confirmation(
        root.path(),
        confirmed_grant(now),
        &[".env.production".to_owned()],
    )
    .unwrap();
    assert!(require_authorized(root.path(), &grant, grant.expires_at_unix_ms).is_err());

    let mut other_project = grant.clone();
    other_project.workspace.project_id = "00000000-0000-4000-8000-000000000148".into();
    assert!(require_authorized(root.path(), &other_project, now + 1).is_err());
}

#[test]
fn active_grant_listing_is_task_scoped_and_hides_expired_or_revoked_receipts() {
    let root = tempfile::tempdir().unwrap();
    let now = crate::persistence::harness_settings::current_unix_ms();
    let grant = record_user_confirmation(
        root.path(),
        confirmed_grant(now),
        &[".env.production".to_owned()],
    )
    .unwrap();
    let active = active_for_task(
        root.path(),
        &grant.workspace.project_id,
        &grant.workspace.task_id,
        now + 1,
    )
    .unwrap();
    assert_eq!(active, vec![grant.clone()]);
    assert!(
        active_for_task(
            root.path(),
            &grant.workspace.project_id,
            "00000000-0000-4000-8000-000000000149",
            now + 1,
        )
        .unwrap()
        .is_empty()
    );
    assert!(
        active_for_task(
            root.path(),
            &grant.workspace.project_id,
            &grant.workspace.task_id,
            grant.expires_at_unix_ms,
        )
        .unwrap()
        .is_empty()
    );

    revoke(
        root.path(),
        &grant.workspace.project_id,
        &grant.grant_id,
        now + 2,
        PrivateFileGrantRevocationReasonV1::OperatorRevoked,
    )
    .unwrap();
    assert!(
        active_for_task(
            root.path(),
            &grant.workspace.project_id,
            &grant.workspace.task_id,
            now + 3,
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn receipt_lookup_keeps_revoked_metadata_available_for_execution_recovery() {
    let root = tempfile::tempdir().unwrap();
    let now = crate::persistence::harness_settings::current_unix_ms();
    let grant = record_user_confirmation(
        root.path(),
        confirmed_grant(now),
        &[".env.production".into()],
    )
    .unwrap();
    revoke(
        root.path(),
        &grant.workspace.project_id,
        &grant.grant_id,
        now + 1,
        PrivateFileGrantRevocationReasonV1::OperatorRevoked,
    )
    .unwrap();

    assert_eq!(
        receipt_by_id(root.path(), &grant.workspace.project_id, &grant.grant_id,).unwrap(),
        grant
    );
}

#[test]
fn active_grant_listing_hides_future_dated_receipts() {
    let root = tempfile::tempdir().unwrap();
    let now = crate::persistence::harness_settings::current_unix_ms();
    let grant = confirmed_grant(now + 10_000).grant;
    let path = store::path(root.path(), &grant.workspace.project_id).unwrap();
    let mut state = GrantStoreV1::empty(&grant.workspace.project_id);
    state.grants.push(grant.clone());
    store::write(&path, &state).unwrap();

    let active = active_for_task(
        root.path(),
        &grant.workspace.project_id,
        &grant.workspace.task_id,
        now,
    )
    .unwrap();
    assert!(active.is_empty());
}

#[test]
fn authorized_result_use_serializes_with_persisted_revocation() {
    // Production boundary: result application and chunk upload hold this
    // project-store lock. This proves a revoke cannot commit while authorized
    // work is inside that guard; pure receipt tests cannot prove OS lock
    // ordering. It uses two file handles and threads, with no external service.
    let root = tempfile::tempdir().unwrap();
    let now = crate::persistence::harness_settings::current_unix_ms();
    let grant = record_user_confirmation(
        root.path(),
        confirmed_grant(now),
        &[".env.production".to_owned()],
    )
    .unwrap();
    let authorized_use =
        lock_authorized_grants(root.path(), std::slice::from_ref(&grant), now + 1).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let revoke_root = root.path().to_path_buf();
    let project_id = grant.workspace.project_id.clone();
    let grant_id = grant.grant_id.clone();
    std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = revoke(
            &revoke_root,
            &project_id,
            &grant_id,
            now + 2,
            PrivateFileGrantRevocationReasonV1::OperatorRevoked,
        );
        done_tx.send(result).unwrap();
    });
    started_rx.recv().unwrap();
    assert!(matches!(
        done_rx.recv_timeout(std::time::Duration::from_millis(50)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    drop(authorized_use);
    done_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert!(require_authorized(root.path(), &grant, now + 3).is_err());
}
