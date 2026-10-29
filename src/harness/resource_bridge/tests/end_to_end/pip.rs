use crate::execution::{
    contract::{ExecutionModeV1, ResourceBudgetV1, WorkspaceRef},
    docker::images::RuntimeProfile,
    session::ExecutionSession,
};
use crate::harness::{
    DependencyDecision, DependencyKind, DependencyNeed, DependencyRequestStatus, PackageEcosystem,
};
use std::{
    fs,
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

/// Proves the production broker-to-session cache handoff and the locked Python executor's
/// network-denied wheel install. Unit tests cannot prove the broker snapshot is extracted at the
/// executor path or that pip can consume it with no index access. It needs rootless Docker and
/// the pinned Python image, with a small fixture setup and container start cost.
#[test]
fn authorized_pip_restore_repairs_cache_for_network_disabled_python_build() {
    if !cfg!(target_os = "linux")
        || std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1")
    {
        return;
    }

    let workspace =
        std::env::temp_dir().join(format!("koolade-pip-restore-e2e-{}", uuid::Uuid::new_v4()));
    let repository = workspace.join("repository");
    let project_root = repository.join("apps/python");
    fs::create_dir_all(&project_root).unwrap();
    super::support::initialize_project(&repository);
    let fixture_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_qa/python_pip_web");
    fs::copy(
        fixture_root.join("requirements.txt"),
        project_root.join("requirements.txt"),
    )
    .unwrap();
    fs::copy(
        fixture_root.join("test_pip_cache.py"),
        project_root.join("test_pip_cache.py"),
    )
    .unwrap();
    super::support::commit(&repository);

    let task_home = workspace.join("task-home");
    let _home = super::support::task_home(&task_home);
    let task_repository =
        super::support::create_task_clone(&repository, &task_home, "pip-restore-e2e");
    let task_project_root = task_repository.join("apps/python");
    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let source_sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&task_repository)
        .output()
        .unwrap();
    assert!(source_sha.status.success());
    let session = Arc::new(
        ExecutionSession::connect_local(
            WorkspaceRef {
                project_id: project_id.clone(),
                task_id: task_id.clone(),
                source_sha: String::from_utf8(source_sha.stdout).unwrap().trim().into(),
            },
            &task_repository,
            ExecutionModeV1::Implementation,
            RuntimeProfile::Python,
            ResourceBudgetV1 {
                cpu_millis: 4_000,
                memory_bytes: 4 * 1024 * 1024 * 1024,
                process_count: 256,
                temporary_disk_bytes: 4 * 1024 * 1024 * 1024,
                output_bytes: 8 * 1024 * 1024,
            },
        )
        .unwrap(),
    );

    let state_root = workspace.join("app-state");
    fs::create_dir_all(&state_root).unwrap();
    let downloads = Arc::new(AtomicUsize::new(0));
    let fixture_wheel = fixture_root.join("koolade_runtime_qa_probe-1.0.0-py3-none-any.whl");
    let download_count = Arc::clone(&downloads);
    let operations = crate::harness::resource_bridge::pip::test_preparation_operations(
        move |requirements: &Path, wheelhouse: &Path, remaining| {
            download_count.fetch_add(1, Ordering::Relaxed);
            let requirements = fs::read_to_string(requirements)?;
            anyhow::ensure!(
                requirements.contains("koolade-runtime-qa-probe==1.0.0"),
                "Pip received a different canonical lockfile"
            );
            let target = wheelhouse.join("koolade_runtime_qa_probe-1.0.0-py3-none-any.whl");
            fs::copy(&fixture_wheel, &target)?;
            let bytes = usize::try_from(fs::metadata(target)?.len())?;
            anyhow::ensure!(
                bytes <= remaining,
                "synthetic PyPI wheel exceeds broker quota"
            );
            Ok(bytes)
        },
    );
    let (progress, updates) = std::sync::mpsc::channel();
    let bridge = super::super::super::broker::start_with_state_root_and_pip_operations(
        &task_repository,
        Some(&task_project_root),
        Some(&task_id),
        progress,
        Arc::new(AtomicBool::new(false)),
        &state_root,
        operations,
    )
    .unwrap();
    bridge.bind_execution_session(Arc::clone(&session)).unwrap();
    let need = DependencyNeed {
        ecosystem: PackageEcosystem::Pip,
        package: None,
        version: None,
        source: Some("https://pypi.org".into()),
        command: "python3 -m pip install --no-index --require-hashes --no-deps -r requirements.txt"
            .into(),
        reason: "Restore the task's SHA-256-locked Python web dependency".into(),
        kind: DependencyKind::ExistingRestore,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    };
    let prepared = std::thread::scope(|scope| {
        let request = scope.spawn(|| bridge.request_locked_dependency(need));
        let reviewing = super::super::take_dependency_update(&updates);
        assert_eq!(reviewing.status, DependencyRequestStatus::ManagerReviewing);
        super::super::answer(
            &reviewing,
            DependencyDecision::AuthorizeForTask,
            None,
            "The hash-pinned pure Python fixture package is required for the offline build.",
        );
        request.join().unwrap().unwrap()
    });
    assert_eq!(prepared.status, DependencyRequestStatus::Prepared);
    let telemetry = prepared
        .preparation
        .as_ref()
        .expect("preparation telemetry");
    assert_eq!(telemetry.cache_hits, 0);
    assert_eq!(telemetry.packages_downloaded, 1);
    assert!(telemetry.bytes_downloaded > 0);
    assert_eq!(downloads.load(Ordering::Relaxed), 1);

    let install = session
        .run_tool_command(
            "python3 -m pip install --no-index --require-hashes --no-deps --only-binary=:all: --find-links /home/executor/.cache/koolade/pip/wheels --requirement apps/python/requirements.txt --target apps/python/dist/site-packages",
            Duration::from_secs(300),
        )
        .unwrap();
    assert_eq!(
        install.result.exit_code,
        Some(0),
        "network-disabled offline Pip install failed: {:?}; {}",
        install.result,
        install.stderr
    );
    let verify = session
        .run_tool_command(
            "python3 apps/python/test_pip_cache.py",
            Duration::from_secs(60),
        )
        .unwrap();
    assert_eq!(
        verify.result.exit_code,
        Some(0),
        "installed wheel did not load: {:?}; {}",
        verify.result,
        verify.stderr
    );

    drop(session);
    drop(bridge);
    fs::remove_dir_all(workspace).unwrap();
}
