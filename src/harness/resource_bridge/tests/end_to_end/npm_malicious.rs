use super::support::{self, base64, commit, initialize_project, npm_available};
use super::*;
use crate::execution::{
    contract::{DependencyCacheEcosystemV1, ExecutionModeV1, ResourceBudgetV1, WorkspaceRef},
    docker::images::RuntimeProfile,
    session::ExecutionSession,
};
use crate::harness::{
    DependencyDecision, DependencyKind, DependencyNeed, DependencyRequestStatus, PackageEcosystem,
};
use sha2::{Digest, Sha512};
use std::{
    fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::Command,
    time::Duration,
};

mod fixture;

#[test]
fn authorized_npm_restore_blocks_malicious_install_egress_and_host_secrets() {
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
    fs::create_dir_all(&repository).unwrap();
    initialize_project(&repository);
    let archive = fixture::create_archive(&workspace, &repository);
    let archive_bytes = fs::read(&archive).unwrap();
    let integrity = format!("sha512-{}", base64(&Sha512::digest(&archive_bytes)));
    let registry_url = "https://registry.npmjs.org/archive-helper/-/archive-helper-1.0.0.tgz";
    fs::write(
        repository.join("package.json"),
        serde_json::to_vec(&serde_json::json!({
            "name": "synthetic-task",
            "version": "1.0.0",
            "private": true,
            "dependencies": { "archive-helper": "1.0.0" },
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        repository.join("package-lock.json"),
        serde_json::to_vec(&serde_json::json!({
            "name": "synthetic-task",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "requires": true,
            "packages": {
                "": {
                    "name": "synthetic-task",
                    "version": "1.0.0",
                    "dependencies": { "archive-helper": "1.0.0" },
                },
                "node_modules/archive-helper": {
                    "version": "1.0.0",
                    "resolved": registry_url,
                    "integrity": integrity,
                    "hasInstallScript": true,
                },
            },
        }))
        .unwrap(),
    )
    .unwrap();
    commit(&repository);
    let state_root = workspace.join("app-state");
    fs::create_dir_all(&state_root).unwrap();
    let task_home = workspace.join("task-home");
    let _home = support::task_home_with_host_secret_probe(&task_home);
    let task_repository = support::create_task_clone(&repository, &task_home, "dependency-e2e");
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
    let session = ExecutionSession::connect_local(
        workspace_ref,
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

    let operations = crate::harness::resource_bridge::npm::test_preparation_operations(
        archive,
        registry_url.into(),
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
                package: None,
                version: None,
                source: Some("https://registry.npmjs.org".into()),
                command: "npm ci".into(),
                reason: "Restore the task's locked local helper package".into(),
                kind: DependencyKind::ExistingRestore,
                lockfile_identity: None,
                introduced_packages: Vec::new(),
            }),
            dependency_request_id: None,
            retry_succeeded: None,
            purpose: "Restore the task's declared npm dependencies".into(),
        },
    )
    .unwrap();
    client.write_all(b"\n").unwrap();

    let reviewed = super::super::take_dependency_update(&updates);
    assert_eq!(
        reviewed.status,
        DependencyRequestStatus::ManagerReviewing,
        "unexpected request status: {:?} ({})",
        reviewed.status,
        reviewed.rationale
    );
    assert_eq!(reviewed.need.package, None);
    assert_eq!(reviewed.need.version, None);
    assert_eq!(reviewed.need.command, "npm ci");
    super::super::answer(
        &reviewed,
        DependencyDecision::AuthorizeForTask,
        None,
        "The task's committed npm lockfile is needed for verification.",
    );

    client
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    let response: ResourceResponse = serde_json::from_str(&response).unwrap();
    assert_eq!(response.status, "prepared", "{}", response.summary);
    let prepared = response
        .dependency_request
        .expect("prepared request is attached");
    assert_eq!(prepared.status, DependencyRequestStatus::Prepared);
    assert_eq!(prepared.decision, DependencyDecision::AuthorizeForTask);
    assert!(
        prepared
            .rationale
            .contains("prepared the offline package cache")
    );
    assert!(
        prepared
            .rationale
            .contains("1 lockfile-pinned packages retrieved")
    );
    assert!(
        !task_repository
            .join("fixtures/archive-helper/install-probe.json")
            .exists(),
        "broker cache indexing must not execute package install scripts"
    );
    assert!(!serde_json::to_string(&prepared).unwrap().contains("_auth"));
    let arch = session.cache_target_arch().unwrap();
    let snapshot = bridge
        .cache_snapshot(
            &task_id,
            DependencyCacheEcosystemV1::Npm,
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
            "npm ci --offline --no-audit --no-fund",
            Duration::from_secs(300),
        )
        .unwrap();
    assert_eq!(
        output.result.exit_code,
        Some(0),
        "offline npm restore failed: {:?}; {}",
        output.result,
        output.stderr
    );

    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(task_repository.join("package.json")).unwrap()).unwrap();
    assert_eq!(manifest["dependencies"]["archive-helper"], "1.0.0");
    let lockfile: serde_json::Value =
        serde_json::from_slice(&fs::read(task_repository.join("package-lock.json")).unwrap())
            .unwrap();
    assert_eq!(
        lockfile["packages"]["node_modules/archive-helper"]["integrity"],
        integrity
    );
    assert!(
        task_repository
            .join("node_modules/archive-helper/package.json")
            .is_file()
    );
    let install_probe: serde_json::Value = serde_json::from_slice(
        &fs::read(task_repository.join("node_modules/archive-helper/install-probe.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(install_probe["network"], "blocked");
    assert_eq!(install_probe["hostSecretPresent"], false);
    assert_eq!(
        fs::read_to_string(task_repository.join("node_modules/archive-helper/index.js")).unwrap(),
        "module.exports = 'ready';\n"
    );
    drop(session);
    drop(bridge);
    fs::remove_dir_all(workspace).unwrap();
}
