use super::*;
use crate::harness::{
    DependencyDecision, DependencyKind, DependencyNeed, DependencyRequestStatus, PackageEcosystem,
    pi_sandbox::Sandbox,
};
use sha2::{Digest, Sha512};
use std::{
    fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::Command,
    time::Duration,
};

#[test]
fn authorized_npm_restore_indexes_synthetic_archive_for_offline_install() {
    if !cfg!(target_os = "linux") {
        return;
    }
    assert!(bwrap_available(), "Linux E2E requires bubblewrap");
    assert!(npm_available(), "Linux E2E requires Node.js and npm");
    let workspace =
        std::env::temp_dir().join(format!("koolade-npm-addition-e2e-{}", uuid::Uuid::new_v4()));
    let repository = workspace.join("repository");
    fs::create_dir_all(&repository).unwrap();
    initialize_project(&repository);
    let package_dir = repository.join("fixtures/archive-helper");
    fs::create_dir_all(&package_dir).unwrap();
    fs::write(
        package_dir.join("package.json"),
        r#"{"name":"archive-helper","version":"1.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(package_dir.join("index.js"), "module.exports = 'ready';\n").unwrap();
    let global_config = workspace.join("npm-globalrc");
    fs::write(&global_config, []).unwrap();
    let npm_home = workspace.join("npm-home");
    fs::create_dir_all(&npm_home).unwrap();
    let output = Command::new("npm")
        .args([
            &format!("--globalconfig={}", global_config.display()),
            "--userconfig=/dev/null",
            "--offline",
            "--ignore-scripts",
            "pack",
            "./fixtures/archive-helper",
        ])
        .arg(format!("--pack-destination={}", workspace.display()))
        .current_dir(&repository)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &npm_home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "local npm fixture setup failed: {}; {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let archive = workspace.join("archive-helper-1.0.0.tgz");
    assert!(
        archive.is_file(),
        "npm pack did not create the fixture archive"
    );
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
                },
            },
        }))
        .unwrap(),
    )
    .unwrap();
    commit(&repository);
    let state_root = workspace.join("app-state");
    fs::create_dir_all(&state_root).unwrap();
    let task_repository = workspace
        .join(".koolade-worktrees")
        .join(crate::persistence::project_slug(&repository))
        .join("task worktree");
    fs::create_dir_all(task_repository.parent().unwrap()).unwrap();
    run_git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "koolade-dependency-e2e",
            task_repository.to_str().unwrap(),
        ],
    );

    let operations = crate::harness::resource_bridge::npm::test_preparation_operations(
        archive,
        registry_url.into(),
    );
    let (progress, updates) = std::sync::mpsc::channel();
    let bridge = super::super::broker::start_with_state_root_and_npm_operations(
        &task_repository,
        Some("synthetic-npm-e2e"),
        progress,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        &state_root,
        operations,
    )
    .unwrap();
    let mut sandbox = Sandbox::new(&task_repository).unwrap();
    sandbox
        .mount_npm_cache_with_snapshot(bridge.npm_cache_path(), bridge.npm_index_snapshot_path())
        .unwrap();
    let mut client = UnixStream::connect(bridge.socket_path()).unwrap();
    serde_json::to_writer(
        &mut client,
        &ResourceRequest {
            action: super::super::ResourceAction::DependencyRequest,
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
    client.shutdown(std::net::Shutdown::Write).unwrap();

    let reviewed = super::take_dependency_update(&updates);
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
    super::answer(
        &reviewed.id,
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
    assert!(!serde_json::to_string(&prepared).unwrap().contains("_auth"));
    let command = with_npm_cache_index_staging(
        "test \"$(npm config get globalconfig)\" = /tmp/koolade-home/.npm-globalrc && \
         npm ci --offline --no-audit --no-fund",
    );
    let output = Command::new(&sandbox.bwrap)
        .args(sandbox.command_args("/bin/bash", &command))
        .current_dir(&sandbox.root)
        .env_clear()
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "offline npm restore failed: {}; {}",
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
        lockfile["packages"]["node_modules/archive-helper"]["integrity"],
        integrity
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
    drop(sandbox);
    drop(bridge);
    fs::remove_dir_all(workspace).unwrap();
}

mod addition;
mod support;
use support::{
    base64, bwrap_available, commit, initialize_project, npm_available, run_git,
    with_npm_cache_index_staging,
};
