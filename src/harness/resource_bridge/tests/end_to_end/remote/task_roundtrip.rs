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
    let worker_id = crate::persistence::execution_records::owner_instance_id().unwrap();
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
    let task_id = workspace.task_id.clone();
    crate::persistence::remote_execution_telemetry::set_task_queue_wait_ms(&task_id, Some(17))
        .unwrap();
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
            destination_sha: None,
            authorization_verifier: Some(Arc::new(authorization::TestAuthorization(generation))),
            task_clone: Some(&task_clone),
            runtime_config_source: Some(&repository),
        },
    )
    .expect("remote worker must accept the authorized task snapshot");
    session.set_telemetry_attribution("codex", Some("gpt-5-codex"));

    let output = session
        .run_tool_command(
            "printf 'verified remote result\\n' > remote-result.txt",
            Duration::from_secs(120),
        )
        .expect("remote task command and verified result synchronization must succeed");
    assert_eq!(output.result.exit_code, Some(0));
    // Production boundary: this crosses authenticated SSH, worker Docker,
    // staging, verified result application, and local telemetry persistence.
    // It proves queue wait and execution metrics follow the task identity into
    // the real remote run. A unit test cannot prove that end-to-end binding.
    // It requires Linux, sshd, the worker binary, and rootless Docker, with
    // high setup cost. Provider behavior remains covered by mock tests.
    let telemetry =
        crate::persistence::remote_execution_telemetry::load(&output.result.execution_id)
            .unwrap()
            .expect("remote execution should persist transfer metrics");
    assert!(
        telemetry
            .workspace_upload_bytes
            .is_some_and(|value| value > 0)
    );
    assert!(
        telemetry
            .result_download_bytes
            .is_some_and(|value| value > 0)
    );
    assert_eq!(telemetry.task_id.as_deref(), Some(task_id.as_str()));
    assert_eq!(telemetry.queue_wait_ms, Some(17));
    assert_eq!(telemetry.harness.as_deref(), Some("codex"));
    assert_eq!(telemetry.requested_model.as_deref(), Some("gpt-5-codex"));
    assert!(
        telemetry
            .image_digest
            .as_deref()
            .is_some_and(|digest| { digest.starts_with("sha256:") && digest.len() == 71 })
    );
    assert!(telemetry.execution_wall_ms.is_some());
    let phases = telemetry
        .execution_phase_timings
        .as_ref()
        .expect("worker phase timings should be relayed to controller telemetry");
    assert!(phases.image_preparation_ms.is_some());
    assert!(phases.dependency_cache_stage_ms.is_some());
    assert!(phases.workspace_stage_in_ms.is_some());
    assert!(phases.workspace_stage_out_ms.is_some());
    assert!(phases.cleanup_ms.is_some());
    assert_eq!(
        telemetry.execution_cpu_millis,
        Some(output.result.metrics.cpu_millis)
    );
    assert_eq!(
        telemetry.execution_peak_memory_bytes,
        Some(output.result.metrics.peak_memory_bytes)
    );
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
    crate::persistence::remote_execution_telemetry::clear_task_queue_wait(&task_id);
    drop(sshd);
    fs::remove_dir_all(root).unwrap();
}
