use super::*;
use crate::execution::remote::private_files::{
    PrivateDataClassV1, PrivateFileSpecV1, RemotePrivateFileGrantV1,
    UserConfirmedPrivateFileGrantV1,
};
use sha2::Digest;

/// Production boundary: authenticates over SSH, stages a confirmed private
/// file, bind-mounts it into the rootless Docker executor, checks read-only
/// access under the executor UID, then revokes the grant while the command is
/// active and checks that no stale result is applied. Unit tests cannot prove
/// host bind-mount permissions, SSH-to-Docker behavior, or poll-time revocation.
/// Requires rootless Docker, sshd, and the remote worker binary; setup is
/// moderate (one worker and one task clone).
#[test]
fn ssh_worker_mounts_confirmed_private_file_read_only_for_executor_uid() {
    if !cfg!(target_os = "linux")
        || std::env::var("KOOLADE_REMOTE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1")
    {
        return;
    }
    let worker_binary = std::env::var_os("KOOLADE_TEST_WORKER_BINARY")
        .map(std::path::PathBuf::from)
        .expect("remote Docker E2E requires KOOLADE_TEST_WORKER_BINARY");
    assert!(
        worker_binary.is_file(),
        "remote worker binary is unavailable"
    );

    let root = tempfile::tempdir().unwrap();
    let root_path = root.path();
    let task_home_path = root_path.join("task-home");
    fs::create_dir_all(&task_home_path).unwrap();
    let _home = support::task_home(&task_home_path);
    let repository = root_path.join("repository");
    fs::create_dir_all(&repository).unwrap();
    support::initialize_project(&repository);
    fs::write(
        repository.join("README.md"),
        "synthetic private-file task\n",
    )
    .unwrap();
    support::commit(&repository);
    let task_clone = support::create_task_clone(&repository, &task_home_path, "private-file");

    // Synthetic fixture bytes are never included in the command, stdout,
    // journal, or assertion messages.
    let private_bytes = b"synthetic-secret-fixture\n";
    fs::write(task_clone.join(".env.production"), private_bytes).unwrap();
    let worker_id = crate::persistence::execution_records::owner_instance_id().unwrap();
    let sshd = sshd::SshdFixture::start(root_path, &worker_binary)
        .expect("CI must provide loopback sshd for remote execution E2E");
    sshd.verify_client_authentication().unwrap();
    sshd.restore_known_host(&worker_id).unwrap();
    let worker = sshd.worker_config(&worker_id);
    crate::persistence::remote_workers::register(
        &worker_id,
        &worker.host_alias,
        &worker.ssh_principal,
        &sshd.host_fingerprint,
    )
    .unwrap();

    let source_sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&task_clone)
        .output()
        .unwrap();
    assert!(source_sha.status.success());
    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let workspace = WorkspaceRef {
        project_id: project_id.clone(),
        task_id: task_id.clone(),
        source_sha: String::from_utf8(source_sha.stdout)
            .unwrap()
            .trim()
            .to_owned(),
    };
    let snapshot =
        crate::execution::remote::WorkspaceManifestV1::capture(workspace.clone(), &task_clone)
            .unwrap();
    assert!(
        snapshot
            .excluded_paths
            .iter()
            .any(|path| path == ".env.production")
    );

    let now = crate::persistence::harness_settings::current_unix_ms();
    let grant_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let grant = RemotePrivateFileGrantV1 {
        grant_id: grant_id.clone(),
        controller_id: crate::persistence::execution_records::owner_instance_id().unwrap(),
        operator_id: String::new(),
        worker_id: worker_id.clone(),
        worker_host_key_fingerprint: sshd.host_fingerprint.clone(),
        workspace: workspace.clone(),
        confirmed_at_unix_ms: 0,
        expires_at_unix_ms: now
            + crate::execution::remote::private_files::MAX_PRIVATE_FILE_GRANT_TTL_MS,
        files: vec![PrivateFileSpecV1 {
            path: ".env.production".into(),
            size_bytes: private_bytes.len() as u64,
            sha256: format!("{:x}", sha2::Sha256::digest(private_bytes)),
            data_class: PrivateDataClassV1::CredentialFile,
        }],
    };
    let confirmed = UserConfirmedPrivateFileGrantV1::after_confirmation(
        grant,
        "synthetic-operator".into(),
        now,
    )
    .unwrap();
    crate::persistence::remote_private_file_grants::record_user_confirmation(
        &crate::persistence::state_root(),
        confirmed,
        &snapshot.excluded_paths,
    )
    .unwrap();

    let session = Arc::new(two_controllers::connect_session_with_ids(
        &task_clone,
        &repository,
        &worker,
        Arc::new(authorization::TestAuthorization(
            uuid::Uuid::new_v4().hyphenated().to_string(),
        )),
        &project_id,
        &task_id,
    ));
    let expected_digest = format!("{:x}", sha2::Sha256::digest(private_bytes));
    let command = format!(
        "test \"$(sha256sum /koolade-private-files/.env.production | cut -d' ' -f1)\" = '{expected_digest}' && if printf probe >> /koolade-private-files/.env.production 2>/dev/null; then exit 41; fi && printf 'private-file-read-only-ok\\n' && sleep 30 && printf 'stale-private-result\\n' > remote-private-result.txt"
    );
    let run_session = session.clone();
    let mut controller = Some(std::thread::spawn(move || {
        run_session.run_tool_command(&command, Duration::from_secs(120))
    }));
    two_controllers::wait_for_remote_stdout(
        &session,
        "private-file-read-only-ok",
        &mut controller,
        &root_path.join("sshd.log"),
    );
    crate::persistence::remote_private_file_grants::revoke(
        &crate::persistence::state_root(),
        &project_id,
        &grant_id,
        now + 1,
        crate::persistence::remote_private_file_grants::PrivateFileGrantRevocationReasonV1::OperatorRevoked,
    )
    .unwrap();
    let outcome = controller
        .take()
        .unwrap()
        .join()
        .expect("remote revocation controller should not panic");
    assert!(matches!(
        outcome,
        Err(ref error)
            if error.kind == crate::execution::errors::ExecutionErrorKind::PolicyDenied
    ));
    assert!(
        !task_clone.join("remote-private-result.txt").exists(),
        "revoked execution must not apply its staged result"
    );
    drop(session);
    drop(sshd);
}
