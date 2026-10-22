use super::*;

/// Production boundary: authenticated SSH, worker staging, and rootless Docker
/// must withhold an unapproved credential file. Requires Linux, rootless Docker,
/// sshd, and the worker binary; setup cost is moderate (one worker and clone).
#[test]
fn ssh_worker_never_stages_an_unapproved_private_file() {
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
    let task_home_path = root.path().join("task-home");
    fs::create_dir_all(&task_home_path).unwrap();
    let _home = support::task_home(&task_home_path);
    let repository = root.path().join("repository");
    fs::create_dir_all(&repository).unwrap();
    support::initialize_project(&repository);
    fs::write(
        repository.join("README.md"),
        "synthetic private-file denial\n",
    )
    .unwrap();
    support::commit(&repository);
    let task_clone = support::create_task_clone(&repository, &task_home_path, "private-denial");
    fs::write(
        task_clone.join(".env.production"),
        b"synthetic-unapproved-secret\n",
    )
    .unwrap();

    let worker_id = crate::persistence::execution_records::owner_instance_id().unwrap();
    let sshd = sshd::SshdFixture::start(root.path(), &worker_binary)
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

    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let session = two_controllers::connect_session_with_ids(
        &task_clone,
        &repository,
        &worker,
        Arc::new(authorization::TestAuthorization(
            uuid::Uuid::new_v4().hyphenated().to_string(),
        )),
        &project_id,
        &task_id,
    );
    let output = session
        .run_tool_command(
            "test ! -e .env.production && test ! -e /koolade-private-files/.env.production && printf 'unapproved-private-file-absent\\n'",
            Duration::from_secs(120),
        )
        .expect("remote execution should succeed without access to the unapproved file");
    assert_eq!(output.result.exit_code, Some(0));
    assert!(task_clone.join(".env.production").is_file());
    assert!(
        !root
            .path()
            .join("worker-state/remote-staging/private-files")
            .join(&output.result.execution_id)
            .exists(),
        "worker must not create a private-file stage without an approved grant"
    );
    drop(session);
    drop(sshd);
}
