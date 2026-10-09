use super::support::{
    bwrap_available, commit, create_task_clone, initialize_project, npm_available, task_home,
    with_npm_cache_index_staging,
};
use super::*;
use crate::harness::{
    DependencyAuthorizationSource, DependencyPreparationStatus, DependencyRetryResult,
};
use std::{fs, os::unix::net::UnixStream, process::Command, time::Duration};

#[test]
fn manager_authorized_new_npm_dependency_is_available_to_offline_install() {
    if !cfg!(target_os = "linux") {
        return;
    }
    assert!(bwrap_available(), "Linux E2E requires bubblewrap");
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
        Some("synthetic-npm-addition-e2e"),
        progress,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        &state_root,
        operations,
    )
    .unwrap();
    let mut sandbox = crate::harness::pi_sandbox::Sandbox::new(&task_repository).unwrap();
    sandbox
        .mount_npm_cache_with_snapshot(bridge.npm_cache_path(), bridge.npm_index_snapshot_path())
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

    let command = with_npm_cache_index_staging(
        "npm install --save-exact archive-helper@1.0.0 --offline --no-audit --no-fund",
    );
    let output = Command::new(&sandbox.bwrap)
        .args(sandbox.command_args("/bin/bash", &command))
        .current_dir(&sandbox.root)
        .env_clear()
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "offline npm addition failed: {}; {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(task_repository.join("package.json")).unwrap()).unwrap();
    assert_eq!(manifest["dependencies"]["archive-helper"], "1.0.0");
    let lockfile: serde_json::Value =
        serde_json::from_slice(&fs::read(task_repository.join("package-lock.json")).unwrap())
            .unwrap();
    assert_eq!(
        lockfile["packages"]["node_modules/archive-helper"]["version"],
        "1.0.0"
    );
    assert!(
        task_repository
            .join("node_modules/archive-helper/package.json")
            .is_file()
    );
    assert_eq!(
        fs::read_to_string(task_repository.join("node_modules/archive-helper/index.js")).unwrap(),
        "module.exports = 'ready';\n"
    );

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

    drop(sandbox);
    drop(bridge);
    fs::remove_dir_all(workspace).unwrap();
}
