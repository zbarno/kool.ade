use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::Duration,
};

use crate::{
    execution::{
        DependencyCacheSnapshotStore,
        contract::{DependencyCacheEcosystemV1, WorkspaceRef},
    },
    harness::{
        DependencyDecision, DependencyKind, DependencyNeed, DependencyRequest,
        DependencyRequestStatus,
    },
};

use super::prepared_cache::SeededCache;

pub(super) fn prepare_broker_cache(
    worktree: &std::path::Path,
    workspace: &WorkspaceRef,
) -> anyhow::Result<SeededCache> {
    let state_root = crate::persistence::state_root();
    let (progress, updates) = mpsc::channel();
    let fixture_cache = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/runtime_qa/go_headless/module-cache/pkg/mod");
    let operations = crate::harness::resource_bridge::test_go_preparation_operations(
        move |_workspace, gopath, _reservation| {
            let destination = gopath.join("pkg/mod");
            let result = copy_fixture_tree(&fixture_cache, &destination);
            (0, result)
        },
    );
    let bridge =
        crate::harness::resource_bridge::ResourceBridge::start_with_state_root_and_go_operations(
            worktree,
            worktree,
            &workspace.task_id,
            progress,
            Arc::new(AtomicBool::new(false)),
            &state_root,
            operations,
        )?;
    let need = DependencyNeed {
        ecosystem: crate::harness::PackageEcosystem::Go,
        package: None,
        version: None,
        source: Some("https://proxy.golang.org".into()),
        command: "go mod download all".into(),
        reason: "Restore the task's exact go.mod/go.sum module set before offline Runtime QA."
            .into(),
        kind: DependencyKind::ExistingRestore,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    };
    let prepared = std::thread::scope(|scope| -> anyhow::Result<DependencyRequest> {
        let request_bridge = &bridge;
        let worker =
            scope.spawn(move || request_bridge.request_locked_dependency_for_fixture(need));
        let reviewing = wait_for_status(&updates, DependencyRequestStatus::ManagerReviewing)?;
        answer(
            &reviewing,
            DependencyDecision::RequiresUserAuthorization,
            None,
            "The public Go lockfile restore is required by the reviewed Runtime QA fixture.",
        )?;
        let awaiting_user = wait_for_status(&updates, DependencyRequestStatus::AwaitingUser)?;
        answer(
            &awaiting_user,
            DependencyDecision::UserAuthorizeForTask,
            Some(crate::harness::DependencyAuthorizationScope::Once),
            "Authorize this exact public Go module lockfile restore once for the task fixture.",
        )?;
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("Go broker request thread panicked"))?
            .map_err(anyhow::Error::msg)
    })?;
    let lockfile_identity = prepared
        .need
        .lockfile_identity
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("prepared Go request has no lockfile identity"))?;
    let snapshot = bridge.cache_snapshot(
        &workspace.task_id,
        DependencyCacheEcosystemV1::Go,
        &workspace.project_id,
        lockfile_identity,
        "amd64",
        "local",
    )?;
    snapshot
        .manifest
        .validate_ref(&snapshot.cache_ref, &snapshot.archive)?;
    bridge.validate_dependency_cache_ref(
        &prepared.id,
        &snapshot.cache_ref,
        &workspace.project_id,
        "local",
        "amd64",
    )?;
    DependencyCacheSnapshotStore::new(state_root.join("runtime-qa-dependency-caches"))
        .put(&prepared.id, &snapshot)?;
    bridge.record_dependency_cache_ref(&prepared.id, snapshot.cache_ref.clone())?;
    let request = bridge
        .dependency_request()
        .ok_or_else(|| anyhow::anyhow!("prepared Go broker request disappeared"))?;
    anyhow::ensure!(
        request.id == prepared.id
            && request.task_id == workspace.task_id
            && request.status == DependencyRequestStatus::Prepared
            && request.need.lockfile_identity == prepared.need.lockfile_identity
            && request
                .preparation
                .as_ref()
                .is_some_and(|preparation| preparation.cache_refs.contains(&snapshot.cache_ref)),
        "Go broker request identity changed while retaining its cache snapshot"
    );
    Ok(SeededCache {
        request_id: request.id.clone(),
        snapshot,
        request: Some(request),
    })
}

fn copy_fixture_tree(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Go module cache fixture root is not a regular directory"
    );
    fs::create_dir_all(
        destination
            .parent()
            .expect("module cache destination parent"),
    )?;
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if kind.is_dir() && !kind.is_symlink() {
            copy_fixture_tree(&entry.path(), &target)?;
        } else {
            anyhow::ensure!(
                kind.is_file() && !kind.is_symlink(),
                "Go module cache fixture contains a link or special file"
            );
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn wait_for_status(
    updates: &mpsc::Receiver<crate::harness::LiveProgress>,
    status: DependencyRequestStatus,
) -> anyhow::Result<DependencyRequest> {
    loop {
        let update = updates
            .recv_timeout(Duration::from_secs(120))
            .map_err(|error| anyhow::anyhow!("Go dependency broker update timed out: {error}"))?;
        if let Some(request) = update
            .dependency_requests
            .into_iter()
            .find(|request| request.status == status)
        {
            return Ok(request);
        }
    }
}

fn answer(
    request: &DependencyRequest,
    decision: DependencyDecision,
    scope: Option<crate::harness::DependencyAuthorizationScope>,
    rationale: &str,
) -> anyhow::Result<()> {
    crate::harness::dependency_authorization::answer_diagnostic(
        &request.id,
        &request.task_id,
        &request.need,
        crate::harness::dependency_authorization::DependencyResolution {
            decision,
            scope,
            rationale: rationale.into(),
        },
    )
    .map_err(|error| anyhow::anyhow!("Go fixture authorization failed: {error}"))
}
