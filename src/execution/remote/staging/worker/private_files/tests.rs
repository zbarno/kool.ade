use super::*;
use sha2::Digest;

fn grant() -> RemotePrivateFileGrantV1 {
    RemotePrivateFileGrantV1 {
        grant_id: "00000000-0000-4000-8000-000000000141".into(),
        controller_id: "00000000-0000-4000-8000-000000000142".into(),
        operator_id: "synthetic-worker-test".into(),
        worker_id: "00000000-0000-4000-8000-000000000143".into(),
        worker_host_key_fingerprint: "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
        workspace: crate::execution::contract::WorkspaceRef {
            task_id: "00000000-0000-4000-8000-000000000144".into(),
            project_id: "00000000-0000-4000-8000-000000000145".into(),
            source_sha: "a".repeat(40),
        },
        confirmed_at_unix_ms: 100,
        expires_at_unix_ms: 200,
        files: vec![crate::execution::remote::private_files::PrivateFileSpecV1 {
            path: ".env.production".into(),
            size_bytes: 5,
            sha256: format!("{:x}", sha2::Sha256::digest(b"token")),
            data_class: crate::execution::remote::private_files::PrivateDataClassV1::CredentialFile,
        }],
    }
}

#[test]
#[cfg(unix)]
fn authorized_chunks_stage_read_only_and_cleanup_requires_confirmed_stop() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let staging = PrivateFiles::new(root.path()).unwrap();
    let mut grant = grant();
    grant.files[0].path = "nested/.env.production".into();
    let execution_id = "00000000-0000-4000-8000-000000000155";
    staging
        .authorize(execution_id, &grant.worker_id, grant.clone(), true, 150)
        .unwrap();
    let chunk = PrivateFileChunkV1::new(
        grant.grant_id.clone(),
        "nested/.env.production".into(),
        0,
        b"token".to_vec(),
    )
    .unwrap();
    staging.receive(execution_id, &chunk, 150).unwrap();
    assert!(staging.receive(execution_id, &chunk, 150).is_err());
    let mounted = staging.mount_root(execution_id, 150).unwrap().unwrap();
    let file = mounted.join("nested/.env.production");
    assert_eq!(fs::read(&file).unwrap(), b"token");
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o444
    );
    assert_eq!(
        fs::metadata(&mounted).unwrap().permissions().mode() & 0o777,
        0o555
    );
    assert_eq!(
        fs::metadata(mounted.join("nested"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o555
    );

    staging.mark_started(execution_id).unwrap();
    assert!(staging.cleanup(execution_id, false).is_err());
    staging.cleanup(execution_id, true).unwrap();
    assert!(!mounted.exists());
}

#[test]
fn incomplete_or_expired_grants_never_produce_a_mount() {
    let root = tempfile::tempdir().unwrap();
    let staging = PrivateFiles::new(root.path()).unwrap();
    let incomplete_grant = grant();
    let worker_id = incomplete_grant.worker_id.clone();
    let execution_id = "00000000-0000-4000-8000-000000000156";
    staging
        .authorize(execution_id, &worker_id, incomplete_grant, true, 150)
        .unwrap();
    assert!(staging.mount_root(execution_id, 150).is_err());
    staging.cleanup(execution_id, false).unwrap();

    let grant = grant();
    let expired_execution = "00000000-0000-4000-8000-000000000157";
    staging
        .authorize(
            expired_execution,
            &grant.worker_id,
            grant.clone(),
            true,
            150,
        )
        .unwrap();
    let chunk = PrivateFileChunkV1::new(
        grant.grant_id,
        ".env.production".into(),
        0,
        b"token".to_vec(),
    )
    .unwrap();
    staging.receive(expired_execution, &chunk, 150).unwrap();
    assert!(staging.mount_root(expired_execution, 200).is_err());
    staging.cleanup(expired_execution, false).unwrap();
}

#[test]
fn stale_started_stage_is_removed_only_by_post_recovery_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let staging_root = root.path().join("remote-staging");
    fs::create_dir_all(&staging_root).unwrap();
    let staging = PrivateFiles::new(&staging_root).unwrap();
    let grant = grant();
    let execution_id = "00000000-0000-4000-8000-000000000158";
    staging
        .authorize(execution_id, &grant.worker_id, grant.clone(), true, 150)
        .unwrap();
    let chunk = PrivateFileChunkV1::new(
        grant.grant_id,
        ".env.production".into(),
        0,
        b"token".to_vec(),
    )
    .unwrap();
    staging.receive(execution_id, &chunk, 150).unwrap();
    staging.mark_started(execution_id).unwrap();
    assert!(matches!(
        crate::execution::remote::staging::lock_private_file_execution_for_recovery(
            root.path(),
            execution_id
        )
        .unwrap(),
        crate::execution::remote::staging::PrivateFileExecutionRecovery::Active
    ));

    // Model a previous worker session. This method is called only after Docker
    // lifecycle recovery has stopped every execution owned by that session.
    drop(staging);
    let restarted = PrivateFiles::new(&staging_root).unwrap();
    let recovery_lock =
        crate::execution::remote::staging::lock_private_file_execution_for_recovery(
            root.path(),
            execution_id,
        )
        .unwrap();
    assert!(matches!(
        &recovery_lock,
        crate::execution::remote::staging::PrivateFileExecutionRecovery::Orphaned { .. }
    ));
    drop(recovery_lock);
    assert!(restarted.cleanup_recovered().is_ok());
    assert!(!restarted.execution_root(execution_id).unwrap().exists());
    assert!(
        !restarted
            .root
            .join(format!("{execution_id}.started"))
            .exists()
    );
}

#[test]
fn prestart_partial_and_complete_stages_are_removed_after_worker_restart() {
    let root = tempfile::tempdir().unwrap();
    let staging_root = root.path().join("remote-staging");
    fs::create_dir_all(&staging_root).unwrap();
    let staging = PrivateFiles::new(&staging_root).unwrap();
    let partial_grant = grant();
    let partial_id = "00000000-0000-4000-8000-000000000159";
    staging
        .authorize(
            partial_id,
            &partial_grant.worker_id,
            partial_grant.clone(),
            true,
            150,
        )
        .unwrap();
    let partial_chunk = PrivateFileChunkV1::new(
        partial_grant.grant_id,
        ".env.production".into(),
        0,
        b"to".to_vec(),
    )
    .unwrap();
    staging.receive(partial_id, &partial_chunk, 150).unwrap();

    let complete_grant = grant();
    let complete_id = "00000000-0000-4000-8000-000000000160";
    staging
        .authorize(
            complete_id,
            &complete_grant.worker_id,
            complete_grant.clone(),
            true,
            150,
        )
        .unwrap();
    let complete_chunk = PrivateFileChunkV1::new(
        complete_grant.grant_id,
        ".env.production".into(),
        0,
        b"token".to_vec(),
    )
    .unwrap();
    staging.receive(complete_id, &complete_chunk, 150).unwrap();
    let sealed_root = staging.mount_root(complete_id, 150).unwrap().unwrap();
    assert!(!staging.root.join(format!("{partial_id}.started")).exists());
    assert!(!staging.root.join(format!("{complete_id}.started")).exists());

    // Without a durable Start marker, neither stage could have been mounted
    // into a Docker container. Recovery can therefore scrub both safely.
    drop(staging);
    let restarted = PrivateFiles::new(&staging_root).unwrap();
    restarted.cleanup_recovered().unwrap();
    assert!(!restarted.execution_root(partial_id).unwrap().exists());
    assert!(!restarted.execution_root(complete_id).unwrap().exists());
    assert!(!sealed_root.exists());
}

#[test]
fn recovery_skips_prestart_stage_owned_by_another_worker_session() {
    let root = tempfile::tempdir().unwrap();
    let staging_root = root.path().join("remote-staging");
    fs::create_dir_all(&staging_root).unwrap();
    let owner = PrivateFiles::new(&staging_root).unwrap();
    let grant = grant();
    let worker_id = grant.worker_id.clone();
    let execution_id = "00000000-0000-4000-8000-000000000161";
    owner
        .authorize(execution_id, &worker_id, grant, true, 150)
        .unwrap();
    let stage = owner.execution_root(execution_id).unwrap();

    let recovery = PrivateFiles::new(&staging_root).unwrap();
    recovery.cleanup_recovered().unwrap();
    assert!(
        stage.is_dir(),
        "a live worker's stage must survive another session's sweep"
    );

    drop(owner);
    recovery.cleanup_recovered().unwrap();
    assert!(
        !stage.exists(),
        "the stage becomes recoverable after owner exit"
    );
}
