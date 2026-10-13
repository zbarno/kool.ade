use super::support::{commit, create_task_clone, initialize_project, npm_available, task_home};
use super::*;
use crate::execution::{
    contract::{ExecutionModeV1, ResourceBudgetV1, WorkspaceRef},
    docker::images::RuntimeProfile,
    session::ExecutionSession,
};
use crate::harness::{
    DependencyAuthorizationSource, DependencyPreparationStatus, DependencyRetryResult,
};
use std::{fs, io::Write, os::unix::net::UnixStream, process::Command, time::Duration};

mod assertions;

#[test]
fn manager_authorized_new_npm_dependency_is_available_to_offline_install() {
    if !cfg!(target_os = "linux") {
        return;
    }
    if std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1") {
        return;
    }
    assert!(npm_available(), "Linux E2E requires Node.js and npm");

    let workspace =
        std::env::temp_dir().join(format!("koolade-npm-addition-e2e-{}", uuid::Uuid::new_v4()));
    let repository = workspace.join("repository");
    let fixture = workspace.join("archive-helper");
    fs::create_dir_all(&repository).unwrap();
    fs::create_dir_all(&fixture).unwrap();
    initialize_project(&repository);
    fs::write(
        repository.join("package.json"),
        r#"{"name":"synthetic-task","version":"1.0.0","private":true}"#,
    )
    .unwrap();
    commit(&repository);
    let task_home_path = workspace.join("task-home");
    let _home = task_home(&task_home_path);
    let task_repository = create_task_clone(&repository, &task_home_path, "npm-addition-e2e");
    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let source_sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&task_repository)
        .output()
        .unwrap();
    assert!(source_sha.status.success());
    let session = ExecutionSession::connect_local(
        WorkspaceRef {
            project_id: project_id.clone(),
            task_id: task_id.clone(),
            source_sha: String::from_utf8(source_sha.stdout).unwrap().trim().into(),
        },
        &task_repository,
        ExecutionModeV1::Implementation,
        RuntimeProfile::Node,
        ResourceBudgetV1 {
            cpu_millis: 4_000,
            memory_bytes: 4 * 1024 * 1024 * 1024,
            process_count: 256,
            temporary_disk_bytes: 4 * 1024 * 1024 * 1024,
            output_bytes: 8 * 1024 * 1024,
        },
    )
    .unwrap();

    fs::write(
        fixture.join("package.json"),
        r#"{"name":"archive-helper","version":"1.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(fixture.join("index.js"), "module.exports = 'ready';\n").unwrap();
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
        "local npm fixture setup failed: {}; {}",
        String::from_utf8_lossy(&packed.stderr),
        String::from_utf8_lossy(&packed.stdout)
    );
    let archive = workspace.join("archive-helper-1.0.0.tgz");
    assert!(
        archive.is_file(),
        "npm pack did not create the fixture archive"
    );

    let state_root = workspace.join("app-state");
    fs::create_dir_all(&state_root).unwrap();
    let registry = "https://registry.npmjs.org/";
    let operations = crate::harness::resource_bridge::npm::test_addition_preparation_operations(
        archive,
        "archive-helper".into(),
        "1.0.0".into(),
        registry.into(),
    );
    let (progress, updates) = std::sync::mpsc::channel();
    let bridge = super::super::super::broker::start_with_state_root_and_npm_operations(
        &task_repository,
        None,
        Some(&task_id),
        progress,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        &state_root,
        operations,
    )
    .unwrap();
    let mut client = UnixStream::connect(bridge.socket_path()).unwrap();
    serde_json::to_writer(
        &mut client,
        &ResourceRequest {
            action: super::super::super::ResourceAction::DependencyRequest,
            manager: None,
            url: None,
            dependency: Some(DependencyNeed {
                ecosystem: PackageEcosystem::Npm,
                package: Some("archive-helper".into()),
                version: Some("1.0.0".into()),
                source: Some("https://registry.npmjs.org".into()),
                command: "npm install --save-exact archive-helper@1.0.0".into(),
                reason: "Add the helper needed by this task's verification.".into(),
                kind: DependencyKind::NewProjectDependency,
                lockfile_identity: None,
                introduced_packages: Vec::new(),
            }),
            dependency_request_id: None,
            retry_succeeded: None,
            purpose: "Prepare an explicitly reviewed npm project dependency".into(),
        },
    )
    .unwrap();
    client.write_all(b"\n").unwrap();

    let reviewing = super::super::take_dependency_update(&updates);
    assert_eq!(reviewing.status, DependencyRequestStatus::ManagerReviewing);
    assert_eq!(reviewing.need.package.as_deref(), Some("archive-helper"));
    assert_eq!(reviewing.need.version.as_deref(), Some("1.0.0"));
    super::super::answer(
        &reviewing,
        DependencyDecision::AuthorizeForTask,
        None,
        "This exact public package is required by the task verification.",
    );

    client
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut response = String::new();
    std::io::Read::read_to_string(&mut client, &mut response).unwrap();
    let response: ResourceResponse = serde_json::from_str(&response).unwrap();
    assert_eq!(response.status, "prepared", "{}", response.summary);
    let prepared = response
        .dependency_request
        .expect("prepared request is attached");
    assert_eq!(prepared.status, DependencyRequestStatus::Prepared);
    assert_eq!(prepared.decision, DependencyDecision::AuthorizeForTask);
    let telemetry = prepared
        .preparation
        .as_ref()
        .expect("preparation telemetry is attached");
    assert_eq!(
        telemetry.status,
        Some(DependencyPreparationStatus::Prepared)
    );
    assert_eq!(
        telemetry.authorization_source,
        Some(DependencyAuthorizationSource::Manager)
    );
    assert_eq!(telemetry.package_count, 1);
    assert_eq!(telemetry.packages_downloaded, 1);
    assert!(telemetry.bytes_downloaded > 0);
    assert!(
        prepared
            .rationale
            .contains("1 lockfile-pinned packages retrieved")
    );

    let arch = session.cache_target_arch().unwrap();
    let snapshot = bridge
        .cache_snapshot(
            &task_id,
            crate::execution::contract::DependencyCacheEcosystemV1::Npm,
            &project_id,
            prepared
                .need
                .lockfile_identity
                .as_deref()
                .unwrap_or("sha256:fixture-lock"),
            &arch,
            session.target_id(),
        )
        .unwrap();
    session.stage_dependency_cache(snapshot).unwrap();
    let output = session
        .run_tool_command(
            "npm install --save-exact archive-helper@1.0.0 --offline --no-audit --no-fund",
            Duration::from_secs(300),
        )
        .unwrap();
    assert!(
        output.result.exit_code == Some(0),
        "offline npm addition failed: {}; {}",
        output.stderr,
        output.stdout
    );
    assertions::assert_installed(&task_repository);

    let mut retry_client = UnixStream::connect(bridge.socket_path()).unwrap();
    serde_json::to_writer(
        &mut retry_client,
        &ResourceRequest {
            action: super::super::super::ResourceAction::DependencyRetryResult,
            manager: None,
            url: None,
            dependency: None,
            dependency_request_id: Some(prepared.id.clone()),
            retry_succeeded: Some(true),
            purpose: "Record the bounded offline npm retry result".into(),
        },
    )
    .unwrap();
    retry_client.write_all(b"\n").unwrap();
    retry_client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut retry_response = String::new();
    std::io::Read::read_to_string(&mut retry_client, &mut retry_response).unwrap();
    let retry_response: ResourceResponse = serde_json::from_str(&retry_response).unwrap();
    assert_eq!(retry_response.status, "prepared");
    assert_eq!(
        retry_response
            .dependency_result
            .as_ref()
            .and_then(|telemetry| telemetry.retry_result),
        Some(DependencyRetryResult::Succeeded)
    );
    let updated = loop {
        let request = super::super::take_dependency_update(&updates);
        if request.id == prepared.id
            && request
                .preparation
                .as_ref()
                .and_then(|telemetry| telemetry.retry_result)
                .is_some()
        {
            break request;
        }
    };
    assert_eq!(
        updated
            .preparation
            .as_ref()
            .and_then(|telemetry| telemetry.retry_result),
        Some(DependencyRetryResult::Succeeded)
    );

    drop(session);
    drop(bridge);
    fs::remove_dir_all(workspace).unwrap();
}
