use super::*;
use crate::execution::{
    contract::{DependencyCacheEcosystemV1, ExecutionModeV1, ResourceBudgetV1, WorkspaceRef},
    docker::images::RuntimeProfile,
    session::ExecutionSession,
};
use crate::harness::{DependencyDecision, DependencyKind, DependencyNeed};
use sha2::{Digest, Sha512};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    process::Command,
    sync::Arc,
    time::Duration,
};

mod authorization;
mod claim_takeover;
mod sshd;
mod task_roundtrip;
mod two_controllers;
mod worker_restart;

#[test]
fn remote_docker_worker_restores_authorized_npm_cache_without_container_network() {
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
    assert!(npm_available(), "remote npm E2E requires npm");

    let workspace =
        std::env::temp_dir().join(format!("koolade-remote-cache-e2e-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&workspace).unwrap();
    let task_home_path = workspace.join("task-home");
    let _home = support::task_home(&task_home_path);
    let repository = workspace.join("repository");
    let fixture = workspace.join("archive-helper");
    fs::create_dir_all(&repository).unwrap();
    fs::create_dir_all(&fixture).unwrap();
    initialize_project(&repository);
    fs::write(
        fixture.join("package.json"),
        r#"{"name":"archive-helper","version":"1.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(
        fixture.join("index.js"),
        "module.exports = 'remote-ready';\n",
    )
    .unwrap();
    let global_config = workspace.join("npm-globalrc");
    fs::write(&global_config, []).unwrap();
    let npm_home = workspace.join("npm-home");
    fs::create_dir_all(&npm_home).unwrap();
    let packed = Command::new("npm")
        .args([
            &format!("--globalconfig={}", global_config.display()),
            "--userconfig=/dev/null",
            "--offline",
            "--ignore-scripts",
            "pack",
            ".",
        ])
        .arg(format!("--pack-destination={}", workspace.display()))
        .current_dir(&fixture)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &npm_home)
        .output()
        .unwrap();
    assert!(
        packed.status.success(),
        "npm fixture pack failed: {}",
        String::from_utf8_lossy(&packed.stderr)
    );
    let archive = workspace.join("archive-helper-1.0.0.tgz");
    let integrity = format!(
        "sha512-{}",
        support::base64(&Sha512::digest(fs::read(&archive).unwrap()))
    );
    let registry_url = "https://registry.npmjs.org/archive-helper/-/archive-helper-1.0.0.tgz";
    fs::write(repository.join("package.json"), r#"{"name":"synthetic-task","version":"1.0.0","private":true,"dependencies":{"archive-helper":"1.0.0"}}"#).unwrap();
    fs::write(repository.join("package-lock.json"), serde_json::to_vec(&serde_json::json!({
        "name":"synthetic-task", "version":"1.0.0", "lockfileVersion":3, "requires":true,
        "packages":{"":{"name":"synthetic-task","version":"1.0.0","dependencies":{"archive-helper":"1.0.0"}},
        "node_modules/archive-helper":{"version":"1.0.0","resolved":registry_url,"integrity":integrity}}
    })).unwrap()).unwrap();
    commit(&repository);

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
    let fingerprint = sshd.host_fingerprint.clone();
    let worker = sshd.worker_config(&worker_id);
    crate::persistence::remote_workers::register(
        &worker_id,
        &worker.host_alias,
        &worker.ssh_principal,
        &fingerprint,
    )
    .unwrap();

    // Verify the two SSH authentication boundaries against a real daemon before
    // exercising the full remote worker protocol below.
    let verified = worker.verify_known_host().unwrap();
    let mut wrong_key = worker.clone();
    wrong_key.verified_host_key_fingerprint = format!("SHA256:{}", "A".repeat(43));
    assert!(wrong_key.verify_known_host().is_err());
    sshd.write_wrong_known_host(&worker_id).unwrap();
    let host_key_failure = sshd.run_worker_command(&verified);
    assert!(!host_key_failure.status.success());
    assert!(
        String::from_utf8_lossy(&host_key_failure.stderr).contains("Host key verification failed")
    );
    sshd.restore_known_host(&worker_id).unwrap();
    let auth_failure = sshd.run_worker_command(&sshd.unauthenticated_worker(&worker));
    assert!(!auth_failure.status.success());
    assert!(String::from_utf8_lossy(&auth_failure.stderr).contains("Permission denied"));

    let task_repository =
        support::create_task_clone(&repository, &task_home_path, "remote-npm-e2e");
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
    let target = crate::execution::contract::ExecutionTarget::remote(worker_id.clone());
    let session = Arc::new(
        ExecutionSession::connect_remote_with_task_clone(
            workspace_ref.clone(),
            crate::execution::session::RemoteSessionOptions {
                target,
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
                authorization_verifier: Some(Arc::new(authorization::TestAuthorization(
                    generation.clone(),
                ))),
                task_clone: Some(&task_repository),
            },
        )
        .unwrap(),
    );
    let staged = session
        .run_bounded_script("true", Duration::from_secs(30))
        .expect("remote task clone should stage before dependency preparation");
    assert_eq!(staged.result.exit_code, Some(0));

    let state_root = workspace.join("app-state");
    fs::create_dir_all(&state_root).unwrap();
    let operations = crate::harness::resource_bridge::npm::test_preparation_operations(
        archive,
        registry_url.into(),
    );
    let (progress, updates) = std::sync::mpsc::channel();
    let bridge = Arc::new(
        super::super::super::broker::start_with_state_root_and_npm_operations(
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
    let response = std::thread::scope(|scope| {
        let relay_thread = scope.spawn(|| {
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
                    reason: "Restore locked package for remote offline verification".into(),
                    kind: DependencyKind::ExistingRestore,
                    // The broker computes and validates lock identity from this
                    // staged task clone; callers cannot supply trusted metadata.
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
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("remote dependency activity channel closed before the request arrived");
                }
            }
            if relay_thread.is_finished() {
                match relay_thread.join().unwrap() {
                    Ok(response) => panic!(
                        "remote dependency preparation completed without emitting a review request: {:?}",
                        response.broker_response
                    ),
                    Err(error) => panic!(
                        "remote dependency preparation failed before emitting a review request: {error:#}"
                    ),
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "remote dependency preparation stayed active without emitting a review request"
            );
        };
        if matches!(
            review.status,
            crate::harness::DependencyRequestStatus::ManagerReviewing
                | crate::harness::DependencyRequestStatus::AwaitingUser
        ) {
            super::super::answer(
                &review,
                DependencyDecision::AuthorizeForTask,
                None,
                "Fixture package is required for offline verification.",
            );
        }
        relay_thread.join().unwrap().unwrap()
    });
    assert_eq!(response.request.worker_id, worker_id);
    assert_eq!(response.request.task_id, task_id);
    assert_eq!(response.request.workspace, workspace_ref);
    assert_eq!(response.request.authorization_generation, generation);
    assert_eq!(response.broker_response["status"], "prepared");
    let dependency_request_id = response.broker_response["dependency_request"]["id"]
        .as_str()
        .expect("prepared broker result must return its request identity");
    let mut broker = UnixStream::connect(bridge.socket_path()).unwrap();
    let cache_request = crate::harness::resource_bridge::CacheSnapshotRequest {
        action: "cache_snapshot".into(),
        dependency_request_id: dependency_request_id.into(),
        ecosystem: DependencyCacheEcosystemV1::Npm,
        project_id,
        target_arch: session
            .cache_target_arch()
            .as_deref()
            .unwrap_or("amd64")
            .into(),
        target_id: session.target_id().into(),
    };
    serde_json::to_writer(&mut broker, &cache_request).unwrap();
    broker.write_all(b"\n").unwrap();
    let mut broker = BufReader::new(broker);
    let mut header = Vec::new();
    broker.read_until(b'\n', &mut header).unwrap();
    let response: crate::harness::resource_bridge::CacheSnapshotResponse =
        serde_json::from_slice(&header).unwrap();
    assert_eq!(response.status, "snapshot");
    let mut archive = vec![0; response.archive_bytes];
    broker.read_exact(&mut archive).unwrap();
    response
        .manifest
        .validate_ref(&response.cache_ref, &archive)
        .unwrap();
    let snapshot = crate::harness::resource_bridge::CacheSnapshot {
        cache_ref: response.cache_ref,
        manifest: response.manifest,
        archive,
    };
    session
        .stage_dependency_cache(snapshot)
        .unwrap_or_else(|error| {
            let sshd_log = fs::read_to_string(workspace.join("sshd.log"))
                .unwrap_or_else(|read_error| format!("sshd log unavailable: {read_error}"));
            panic!("remote cache staging failed: {error}; sshd log:\n{sshd_log}");
        });
    let output = session
        .run_tool_command(
            "npm ci --offline --no-audit --no-fund",
            Duration::from_secs(300),
        )
        .unwrap();
    assert_eq!(
        output.result.exit_code,
        Some(0),
        "remote offline npm restore failed: {}",
        output.stderr
    );
    assert!(
        task_repository
            .join("node_modules/archive-helper/package.json")
            .is_file()
    );
    assert_eq!(
        fs::read_to_string(task_repository.join("node_modules/archive-helper/index.js")).unwrap(),
        "module.exports = 'remote-ready';\n"
    );
    drop(session);
    drop(bridge);
    fs::remove_dir_all(workspace).unwrap();
}
