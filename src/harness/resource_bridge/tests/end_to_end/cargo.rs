use super::*;
use crate::execution::{
    contract::{DependencyCacheEcosystemV1, ExecutionModeV1, ResourceBudgetV1, WorkspaceRef},
    docker::images::RuntimeProfile,
    session::ExecutionSession,
};
use crate::harness::{
    DependencyDecision, DependencyKind, DependencyNeed, DependencyRequestStatus, PackageEcosystem,
};
use std::{
    fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

mod support;

#[test]
fn authorized_cargo_restore_repairs_cache_for_network_disabled_docker_build() {
    if !cfg!(target_os = "linux")
        || std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1")
    {
        return;
    }

    let workspace = std::env::temp_dir().join(format!(
        "koolade-cargo-restore-e2e-{}",
        uuid::Uuid::new_v4()
    ));
    let repository = workspace.join("repository");
    fs::create_dir_all(&repository).unwrap();
    super::support::initialize_project(&repository);
    let package = support::CrateFixture::new();
    package.write_project(&repository);
    super::support::commit(&repository);

    let task_home = workspace.join("task-home");
    let _home = super::support::task_home(&task_home);
    let task_repository =
        super::support::create_task_clone(&repository, &task_home, "cargo-restore-e2e");
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
        RuntimeProfile::Rust,
        ResourceBudgetV1 {
            cpu_millis: 4_000,
            memory_bytes: 4 * 1024 * 1024 * 1024,
            process_count: 256,
            temporary_disk_bytes: 4 * 1024 * 1024 * 1024,
            output_bytes: 8 * 1024 * 1024,
        },
    )
    .unwrap();

    let state_root = workspace.join("app-state");
    fs::create_dir_all(&state_root).unwrap();
    let fetches = Arc::new(AtomicUsize::new(0));
    let fetch_fixture = support::CrateFixture::new();
    let fetch_count = Arc::clone(&fetches);
    let operations = crate::harness::resource_bridge::cargo::test_preparation_operations(
        move |_worktree, _cargo, cargo_home, remaining| {
            fetch_count.fetch_add(1, Ordering::Relaxed);
            let bytes = fetch_fixture.write_registry_cache(cargo_home);
            anyhow::ensure!(
                bytes <= remaining,
                "synthetic registry exceeds the broker quota"
            );
            Ok(bytes)
        },
    );
    let (progress, updates) = std::sync::mpsc::channel();
    let bridge = super::super::super::broker::start_with_state_root_and_cargo_operations(
        &task_repository,
        Some(&task_id),
        progress,
        Arc::new(AtomicBool::new(false)),
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
                ecosystem: PackageEcosystem::Cargo,
                package: None,
                version: None,
                source: Some("https://index.crates.io".into()),
                command: "cargo build --locked --offline".into(),
                reason: "Restore the task's checksum-pinned Rust helper crate".into(),
                kind: DependencyKind::ExistingRestore,
                lockfile_identity: None,
                introduced_packages: Vec::new(),
            }),
            dependency_request_id: None,
            retry_succeeded: None,
            purpose: "Restore the committed Cargo.lock without worker network access".into(),
        },
    )
    .unwrap();
    client.write_all(b"\n").unwrap();

    let reviewing = super::super::take_dependency_update(&updates);
    assert_eq!(reviewing.status, DependencyRequestStatus::ManagerReviewing);
    super::super::answer(
        &reviewing,
        DependencyDecision::AuthorizeForTask,
        None,
        "This committed crates.io package is required to build the task offline.",
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
        .expect("prepared Cargo request is attached");
    assert_eq!(prepared.status, DependencyRequestStatus::Prepared);
    let telemetry = prepared
        .preparation
        .as_ref()
        .expect("preparation telemetry");
    assert_eq!(telemetry.cache_hits, 0);
    assert_eq!(telemetry.packages_downloaded, 1);
    assert!(telemetry.bytes_downloaded > 0);
    assert_eq!(fetches.load(Ordering::Relaxed), 1);

    let snapshot = bridge
        .cache_snapshot(
            &task_id,
            DependencyCacheEcosystemV1::Cargo,
            &project_id,
            prepared.need.lockfile_identity.as_deref().unwrap(),
            &session.cache_target_arch().unwrap(),
            session.target_id(),
        )
        .unwrap();
    session.stage_dependency_cache(snapshot).unwrap();
    let build = session
        .run_tool_command("cargo build --locked --offline", Duration::from_secs(300))
        .unwrap();
    assert_eq!(
        build.result.exit_code,
        Some(0),
        "network-disabled offline Cargo build failed: {:?}; {}",
        build.result,
        build.stderr
    );

    drop(session);
    drop(bridge);
    fs::remove_dir_all(workspace).unwrap();
}
