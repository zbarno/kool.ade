use super::super::*;
use std::sync::mpsc;

/// Production boundary: verifies an authenticated worker's dependency request
/// traverses the controller broker and cannot receive cache bytes after the
/// operator denies it. Unit tests cannot prove the SSH relay and worker cache
/// boundary together. Requires rootless Docker, loopback sshd, npm, and the
/// remote worker binary; setup is moderate (one worker and task clone).
#[test]
fn denied_remote_dependency_request_never_stages_or_installs_cache() {
    if !cfg!(target_os = "linux")
        || std::env::var("KOOLADE_REMOTE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1")
    {
        return;
    }
    let worker_binary = std::env::var_os("KOOLADE_TEST_WORKER_BINARY")
        .map(std::path::PathBuf::from)
        .expect("remote Docker E2E requires KOOLADE_TEST_WORKER_BINARY");
    assert!(worker_binary.is_file());
    assert!(npm_available(), "remote npm E2E requires npm");

    let workspace = std::env::temp_dir().join(format!(
        "koolade-remote-denied-cache-e2e-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&workspace).unwrap();
    let (task_home_path, repository, archive, registry_url) =
        super::setup::prepare_workspace(&workspace);
    let _home = support::task_home(&task_home_path);
    let worker_id = crate::persistence::execution_records::owner_instance_id().unwrap();
    let sshd = match sshd::SshdFixture::start(&workspace, &worker_binary) {
        Ok(fixture) => fixture,
        Err(error)
            if std::env::var("KOOLADE_REQUIRE_SSHD_E2E").as_deref() != Ok("1")
                && error.kind() == std::io::ErrorKind::NotFound =>
        {
            eprintln!("skipping real SSH remote E2E; sshd is unavailable: {error}");
            let _ = fs::remove_dir_all(&workspace);
            return;
        }
        Err(error) => panic!("could not start loopback sshd for remote E2E: {error}"),
    };
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

    let task_repository = support::create_task_clone(&repository, &task_home_path, "denied-cache");
    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let source_sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&task_repository)
        .output()
        .unwrap();
    assert!(source_sha.status.success());
    let workspace_ref = WorkspaceRef {
        project_id: project_id.clone(),
        task_id: task_id.clone(),
        source_sha: String::from_utf8(source_sha.stdout).unwrap().trim().into(),
    };
    let generation = uuid::Uuid::new_v4().hyphenated().to_string();
    let session = Arc::new(
        ExecutionSession::connect_remote_with_task_clone(
            workspace_ref.clone(),
            crate::execution::session::RemoteSessionOptions {
                target: crate::execution::contract::ExecutionTarget::remote(worker_id.clone()),
                worker: &worker,
                mode: ExecutionModeV1::Implementation,
                profile: RuntimeProfile::Node,
                resources: ResourceBudgetV1 {
                    cpu_millis: 4_000,
                    memory_bytes: 4 * 1024 * 1024 * 1024,
                    process_count: 256,
                    temporary_disk_bytes: 4 * 1024 * 1024 * 1024,
                    output_bytes: 8 * 1024 * 1024,
                },
                destination_sha: None,
                authorization_verifier: Some(Arc::new(authorization::TestAuthorization(
                    generation.clone(),
                ))),
                task_clone: Some(&task_repository),
                runtime_config_source: Some(&repository),
            },
        )
        .unwrap(),
    );
    assert_eq!(
        session
            .run_bounded_script("true", Duration::from_secs(30))
            .unwrap()
            .result
            .exit_code,
        Some(0)
    );

    let state_root = workspace.join("app-state");
    fs::create_dir_all(&state_root).unwrap();
    let operations = crate::harness::resource_bridge::npm::test_preparation_operations(
        archive,
        registry_url.into(),
    );
    let (progress, updates) = mpsc::channel();
    let bridge = Arc::new(
        crate::harness::resource_bridge::broker::start_with_state_root_and_npm_operations(
            &task_repository,
            None,
            Some(&task_id),
            progress,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            &state_root,
            operations,
        )
        .unwrap(),
    );
    let request_thread = std::thread::scope(|scope| {
        let relay = scope.spawn(|| {
            crate::harness::execution_security::request_remote_dependency_over_application_bridge(
                &state_root,
                session.clone(),
                task_id.clone(),
                Some(bridge.clone()),
                DependencyNeed {
                    ecosystem: PackageEcosystem::Npm,
                    package: None,
                    version: None,
                    source: Some("https://registry.npmjs.org".into()),
                    command: "npm ci".into(),
                    reason: "Verify denied cache preparation cannot reach a remote worker".into(),
                    kind: DependencyKind::ExistingRestore,
                    lockfile_identity: None,
                    introduced_packages: Vec::new(),
                },
            )
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        let review = loop {
            match updates.recv_timeout(Duration::from_millis(250)) {
                Ok(update) => {
                    if let Some(request) = update.dependency_requests.into_iter().find(|request| {
                        matches!(
                            request.status,
                            crate::harness::DependencyRequestStatus::ManagerReviewing
                                | crate::harness::DependencyRequestStatus::AwaitingUser
                        )
                    }) {
                        break request;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("remote dependency activity channel closed before review");
                }
            }
            if relay.is_finished() {
                match relay.join().unwrap() {
                    Ok(response) => panic!(
                        "remote dependency request completed before operator review: {:?}",
                        response.broker_response
                    ),
                    Err(error) => {
                        panic!("remote dependency request failed before operator review: {error:#}")
                    }
                }
            }
            assert!(std::time::Instant::now() < deadline);
        };
        super::super::super::super::answer(
            &review,
            DependencyDecision::Reject,
            None,
            "Fixture policy denies remote cache transfer.",
        );
        relay.join().unwrap().unwrap()
    });
    assert_eq!(request_thread.broker_response["status"], "denied");
    assert!(
        !task_repository
            .join("node_modules/archive-helper/package.json")
            .exists(),
        "a denied dependency must not be installed in the remote task clone"
    );
    drop(session);
    drop(bridge);
    drop(sshd);
    fs::remove_dir_all(workspace).unwrap();
}
