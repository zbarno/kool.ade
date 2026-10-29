use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::Duration,
};

use crate::{
    app::session::Project,
    core::lifecycle::TaskRecord,
    execution::{
        DependencyCacheSnapshotStore,
        contract::{DependencyCacheEcosystemV1, WorkspaceRef},
    },
    harness::{
        DependencyAuthorizationSource, DependencyDecision, DependencyFailureCategory,
        DependencyKind, DependencyNeed, DependencyPreparationStatus,
        DependencyPreparationTelemetry, DependencyRequest, DependencyRequestStatus,
    },
};

pub(in crate::execution::docker::qa_container::tests::browser_rootless::task_runtime) struct SeededCache
{
    pub request_id: String,
    pub snapshot: crate::harness::resource_bridge::CacheSnapshot,
    pub request: Option<DependencyRequest>,
}

pub(in crate::execution::docker::qa_container::tests::browser_rootless::task_runtime) fn seed_synthetic(
    project: &mut Project,
    task: &TaskRecord,
    workspace: &crate::execution::contract::WorkspaceRef,
    cache_root: &Path,
    dependency_request_ids: &[String],
) -> Option<SeededCache> {
    let request_id = dependency_request_ids.first()?.clone();
    fs::create_dir_all(cache_root).unwrap();
    fs::write(
        cache_root.join("package.tgz"),
        b"approved offline task cache",
    )
    .unwrap();
    let lockfile_identity = "sha256:synthetic-task-lock-v1";
    let snapshot = crate::harness::resource_bridge::CacheSnapshot::capture(
        cache_root,
        &workspace.task_id,
        DependencyCacheEcosystemV1::Npm,
        &workspace.project_id,
        lockfile_identity,
        "amd64",
        "local",
    )
    .unwrap();
    let store = DependencyCacheSnapshotStore::new(
        crate::persistence::state_root().join("runtime-qa-dependency-caches"),
    );
    for id in dependency_request_ids {
        store.put(id, &snapshot).unwrap();
    }

    let task_uid = task.planning_item_uid.as_deref().unwrap();
    assert_eq!(task_uid, workspace.task_id);
    let document_path = project
        .task_documents
        .iter()
        .find(|document| {
            document
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.uid == task_uid)
        })
        .expect("prepared dependency belongs to a persisted task document")
        .path
        .clone();
    let requests = dependency_request_ids
        .iter()
        .map(|id| DependencyRequest {
            id: id.clone(),
            task_id: task_uid.to_owned(),
            need: DependencyNeed {
                ecosystem: crate::harness::PackageEcosystem::Npm,
                package: Some("fixture-package-set".into()),
                version: Some("1.0.0".into()),
                source: None,
                command: "npm ci --offline".into(),
                reason: "restore the reviewed task lockfile without network access".into(),
                kind: DependencyKind::ExistingRestore,
                lockfile_identity: Some(lockfile_identity.into()),
                introduced_packages: Vec::new(),
            },
            category: DependencyFailureCategory::DependencyRestoreRequired,
            decision: DependencyDecision::UserAuthorizeForTask,
            rationale: "the task has a reviewed, task-scoped broker cache".into(),
            risk: "no package source outside the retained broker cache is available".into(),
            status: DependencyRequestStatus::Prepared,
            preparation: Some(DependencyPreparationTelemetry {
                status: Some(DependencyPreparationStatus::Prepared),
                broker_duration_millis: None,
                package_count: 1,
                cache_hits: 1,
                packages_downloaded: 0,
                bytes_downloaded: 0,
                authorization_source: Some(DependencyAuthorizationSource::User),
                retry_result: Some(crate::harness::DependencyRetryResult::Succeeded),
                cache_refs: vec![snapshot.cache_ref.clone()],
            }),
        })
        .collect();
    project
        .activity
        .tasks
        .entry(document_path)
        .or_default()
        .dependency_requests = requests;

    Some(SeededCache {
        request_id,
        snapshot,
        request: None,
    })
}

pub(in crate::execution::docker::qa_container::tests::browser_rootless::task_runtime) fn prepare_broker_cache(
    worktree: &Path,
    workspace: &WorkspaceRef,
    archive: &super::npm_fixture::NpmFixtureArchive,
) -> anyhow::Result<SeededCache> {
    let state_root = crate::persistence::state_root();
    let (progress, updates) = mpsc::channel();
    let bridge = crate::harness::resource_bridge::ResourceBridge::start_with_npm_fixture_archive(
        worktree,
        &workspace.task_id,
        progress,
        Arc::new(AtomicBool::new(false)),
        &state_root,
        crate::harness::resource_bridge::NpmFixtureArchive {
            archive: archive.archive.clone(),
            archive_url: archive.archive_url.clone(),
            registry_url: archive.registry_url.clone(),
        },
    )?;
    let need = DependencyNeed {
        ecosystem: crate::harness::PackageEcosystem::Npm,
        package: None,
        version: None,
        source: Some(archive.registry_url.clone()),
        command: "npm ci --offline --ignore-scripts --no-audit --no-fund".into(),
        reason: "Restore the checked task lockfile for the network-denied Runtime QA build.".into(),
        kind: DependencyKind::ExistingRestore,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    };
    let prepared = std::thread::scope(|scope| -> anyhow::Result<DependencyRequest> {
        let request_bridge = &bridge;
        let worker =
            scope.spawn(move || request_bridge.request_locked_dependency_for_fixture(need));
        let reviewing =
            wait_for_request_status(&updates, DependencyRequestStatus::ManagerReviewing)?;
        answer_request(
            &reviewing,
            DependencyDecision::RequiresUserAuthorization,
            None,
            "The public locked restore is required by the reviewed Runtime QA fixture.",
        )?;
        let awaiting_user =
            wait_for_request_status(&updates, DependencyRequestStatus::AwaitingUser)?;
        answer_request(
            &awaiting_user,
            DependencyDecision::UserAuthorizeForTask,
            Some(crate::harness::DependencyAuthorizationScope::Once),
            "Authorize this exact public lockfile restore once for the task fixture.",
        )?;
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("npm broker request thread panicked"))?
            .map_err(anyhow::Error::msg)
    })?;

    let lockfile_identity = prepared
        .need
        .lockfile_identity
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("prepared npm request has no lockfile identity"))?;
    let snapshot = bridge.cache_snapshot(
        &workspace.task_id,
        DependencyCacheEcosystemV1::Npm,
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
        .ok_or_else(|| anyhow::anyhow!("prepared npm broker request disappeared"))?;
    anyhow::ensure!(
        request.id == prepared.id
            && request.task_id == workspace.task_id
            && request.status == DependencyRequestStatus::Prepared
            && request.need.lockfile_identity == prepared.need.lockfile_identity
            && request
                .preparation
                .as_ref()
                .is_some_and(|preparation| preparation.cache_refs.contains(&snapshot.cache_ref)),
        "broker request identity changed while retaining its cache snapshot"
    );
    Ok(SeededCache {
        request_id: request.id.clone(),
        snapshot,
        request: Some(request),
    })
}

pub(in crate::execution::docker::qa_container::tests::browser_rootless::task_runtime) fn attach_broker_request(
    project: &mut Project,
    task: &TaskRecord,
    request: &DependencyRequest,
) {
    let task_uid = task.planning_item_uid.as_deref().unwrap();
    assert_eq!(task_uid, request.task_id);
    let document_path = project
        .task_documents
        .iter()
        .find(|document| {
            document
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.uid == task_uid)
        })
        .expect("prepared dependency belongs to a persisted task document")
        .path
        .clone();
    project
        .activity
        .tasks
        .entry(document_path)
        .or_default()
        .dependency_requests = vec![request.clone()];
}

fn wait_for_request_status(
    updates: &mpsc::Receiver<crate::harness::LiveProgress>,
    status: DependencyRequestStatus,
) -> anyhow::Result<DependencyRequest> {
    loop {
        let update = updates
            .recv_timeout(Duration::from_secs(120))
            .map_err(|error| anyhow::anyhow!("dependency broker update timed out: {error}"))?;
        if let Some(request) = update
            .dependency_requests
            .into_iter()
            .find(|request| request.status == status)
        {
            return Ok(request);
        }
    }
}

fn answer_request(
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
    .map_err(|error| anyhow::anyhow!("dependency fixture authorization failed: {error}"))
}
