use super::*;

/// Exercises the complete authenticated SSH, Docker execution, and verified
/// workspace result path. CI requires this test to run with rootless Docker.
#[test]
fn ssh_worker_applies_verified_remote_file_change_to_the_task_clone() {
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

    let root =
        std::env::temp_dir().join(format!("koolade-remote-roundtrip-{}", uuid::Uuid::new_v4()));
    let repository = root.join("repository");
    fs::create_dir_all(&repository).unwrap();
    support::initialize_project(&repository);
    fs::write(repository.join("README.md"), "synthetic remote task\n").unwrap();
    support::commit(&repository);

    let task_home_path = root.join("task-home");
    fs::create_dir_all(&task_home_path).unwrap();
    let _home = support::task_home(&task_home_path);
    let task_clone = support::create_task_clone(&repository, &task_home_path, "remote-roundtrip");
    let worker_id = format!("worker-{}", uuid::Uuid::new_v4().simple());
    let sshd = sshd::SshdFixture::start(&root, &worker_binary)
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

    let source_sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&task_clone)
        .output()
        .unwrap();
    assert!(source_sha.status.success());
    let workspace = WorkspaceRef {
        project_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        task_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        source_sha: String::from_utf8(source_sha.stdout)
            .unwrap()
            .trim()
            .to_owned(),
    };
    let generation = uuid::Uuid::new_v4().hyphenated().to_string();
    let session = ExecutionSession::connect_remote_with_task_clone(
        workspace,
        crate::execution::session::RemoteSessionOptions {
            target: crate::execution::contract::ExecutionTarget::remote(worker_id),
            worker: &worker,
            mode: ExecutionModeV1::Implementation,
            profile: RuntimeProfile::General,
            resources: ResourceBudgetV1 {
                cpu_millis: 2_000,
                memory_bytes: 2 * 1024 * 1024 * 1024,
                process_count: 128,
                temporary_disk_bytes: 1024 * 1024 * 1024,
                output_bytes: 1024 * 1024,
            },
            authorization_verifier: Some(Arc::new(authorization::TestAuthorization(generation))),
            task_clone: Some(&task_clone),
        },
    )
    .expect("remote worker must accept the authorized task snapshot");

    let output = session
        .run_tool_command(
            "printf 'verified remote result\\n' > remote-result.txt",
            Duration::from_secs(120),
        )
        .expect("remote task command and verified result synchronization must succeed");
    assert_eq!(output.result.exit_code, Some(0));
    let changed_file = task_clone.join("remote-result.txt");
    assert_eq!(
        fs::read(&changed_file).unwrap(),
        b"verified remote result\n"
    );
    let status = std::process::Command::new("git")
        .args(["status", "--short", "--", "remote-result.txt"])
        .current_dir(&task_clone)
        .output()
        .unwrap();
    assert!(status.status.success());
    assert_eq!(
        String::from_utf8(status.stdout).unwrap().trim(),
        "?? remote-result.txt"
    );
    drop(session);
    drop(sshd);
    fs::remove_dir_all(root).unwrap();
}
