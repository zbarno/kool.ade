use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use sha2::Digest as _;

use super::super::request::PreparedRequest;
use super::reviewer;
#[cfg(test)]
use crate::runtime_qa::QaValidatedTargetV1;
use crate::{
    execution::{
        contract::{ExecutionModeV1, ResourceBudgetV1},
        docker::{
            images::{ImageLock, RuntimeProfile},
            qa_container::{QaArtifactMount, QaContainerUpload, snapshot_selected_files},
        },
        session::ExecutionSession,
    },
    runtime_qa::{
        QaPlatform, QaRunRequestV1, QaScenarioV1, QaTargetAuthorization, RuntimeQaOrchestrator,
        RuntimeQaStore,
    },
};

mod execution;

fn prepare_dispatch_artifacts(
    prepared: &PreparedRequest,
) -> Result<(QaRunRequestV1, QaContainerUpload), String> {
    let request = prepared.request.clone();
    let recipe_bytes = serde_json::to_vec(&prepared.startup_recipe)
        .map_err(|error| format!("approved QA startup recipe could not be encoded: {error}"))?;
    let recipe_sha = format!("{:x}", sha2::Sha256::digest(recipe_bytes));
    if recipe_sha != request.startup_recipe_sha256
        || prepared.startup_recipe.recipe_id != request.startup_recipe_ref
    {
        return Err(
            "the approved startup recipe changed after the QA request was persisted".into(),
        );
    }
    let runtime = crate::project_runtime::ProjectRuntimeStore::new(&prepared.planning_store);
    let manifest = runtime
        .load_manifest()
        .map_err(|error| format!("project runtime manifest could not be revalidated: {error}"))?
        .ok_or_else(|| "project runtime manifest disappeared before QA dispatch".to_owned())?;
    super::super::request::verify_prepared_candidate(
        &runtime,
        &manifest,
        &request,
        super::super::request::PreparedCandidate {
            workspace: &prepared.workspace,
            target: &prepared.target,
            startup_recipe: &prepared.startup_recipe,
            readiness_probe: prepared.readiness_probe.as_ref(),
            artifact_paths: &prepared.artifact_paths,
        },
    )?;
    let artifact_bundle_sha = super::super::request::artifact_bundle_sha256(
        &prepared.artifact_paths,
        &prepared.build_receipt.artifact_hashes,
    )?;
    if artifact_bundle_sha != request.build_artifact_sha256 {
        return Err(
            "prepared Runtime QA request no longer matches its approved build receipt".into(),
        );
    }
    let (artifact_upload, _snapshot_sha) = snapshot_selected_files(
        &prepared.task_clone,
        &prepared.artifact_paths,
        &prepared.build_receipt.artifact_hashes,
        request
            .bounded_limits
            .max_evidence_bytes
            .min(512 * 1024 * 1024) as usize,
    )
    .map_err(|error| error.message)?;
    Ok((request, artifact_upload))
}

#[cfg(test)]
pub(crate) struct RootlessDesktopDispatch {
    pub request: QaRunRequestV1,
    pub target: QaValidatedTargetV1,
    pub artifact_upload: QaContainerUpload,
    pub artifact_mount: QaArtifactMount,
}

#[cfg(test)]
pub(crate) fn prepare_rootless_desktop_dispatch(
    project: &crate::app::session::Project,
    task: &crate::core::lifecycle::TaskRecord,
) -> Result<RootlessDesktopDispatch, String> {
    let prepared = super::super::request::prepare(project, task)?;
    let (request, artifact_upload) = prepare_dispatch_artifacts(&prepared)?;
    let target = execution::desktop_target(&prepared, &request)?;
    let artifact_mount = QaArtifactMount::inspect(&prepared.task_clone, "/workspace")
        .map_err(|error| error.message)?;
    Ok(RootlessDesktopDispatch {
        request,
        target,
        artifact_upload,
        artifact_mount,
    })
}

pub(super) fn run(
    prepared: PreparedRequest,
    scenarios: Vec<QaScenarioV1>,
    harness: Box<dyn crate::harness::AiHarness>,
    store_root: PathBuf,
    cancelled: Arc<AtomicBool>,
    mut docker_cancel: tokio::sync::watch::Receiver<bool>,
    _progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
) -> Result<(), String> {
    let (request, artifact_upload) = prepare_dispatch_artifacts(&prepared)?;
    let store = RuntimeQaStore::new(store_root);
    let profile = profile_for_platforms(&request.relevant_platforms)?;
    let resources = ResourceBudgetV1 {
        cpu_millis: 2_000,
        memory_bytes: 4 * 1024 * 1024 * 1024,
        process_count: 128,
        temporary_disk_bytes: 2 * 1024 * 1024 * 1024,
        output_bytes: 64 * 1024 * 1024,
    };
    let session = ExecutionSession::connect_local(
        prepared.workspace.clone(),
        &prepared.task_clone,
        ExecutionModeV1::RuntimeQa,
        profile,
        resources.clone(),
    )
    .map_err(|error| {
        format!(
            "isolated Runtime QA Docker session is unavailable: {}",
            error.message
        )
    })?;
    let architecture = session.cache_target_arch().ok_or_else(|| {
        "selected QA image architecture is not in the reviewed image lock".to_owned()
    })?;
    let dependency_cache_store = crate::execution::DependencyCacheSnapshotStore::new(
        crate::persistence::state_root().join("runtime-qa-dependency-caches"),
    );
    let mut dependency_caches = Vec::new();
    for dependency in &request.dependency_cache_refs {
        if dependency.cache_ref.target_arch != architecture {
            return Err(format!(
                "prepared dependency cache for request `{}` targets a different architecture",
                dependency.dependency_request_id
            ));
        }
        let snapshot = dependency_cache_store
            .get(&dependency.dependency_request_id, &dependency.cache_ref)
            .map_err(|_| {
                format!(
                    "prepared dependency cache for request `{}` is unavailable or failed integrity validation",
                    dependency.dependency_request_id
                )
            })?;
        dependency_caches.push(snapshot);
    }
    let lock = ImageLock::bundled().map_err(|error| error.message)?;
    let qa_image = lock
        .select(profile, &architecture)
        .map_err(|error| error.message)?;
    let artifact_mount = QaArtifactMount::inspect(&prepared.task_clone, "/workspace")
        .map_err(|error| error.message)?;
    match profile {
        RuntimeProfile::BrowserQa => execution::run_service(
            &session,
            &prepared,
            &request,
            &scenarios,
            &artifact_upload,
            artifact_mount,
            &resources,
            &qa_image,
            architecture,
            &dependency_caches,
            &store,
            harness,
            &mut docker_cancel,
            cancelled,
        ),
        RuntimeProfile::DesktopQa => execution::run_desktop(
            &session,
            &prepared,
            &request,
            &scenarios,
            &artifact_upload,
            artifact_mount,
            &resources,
            &qa_image,
            &dependency_caches,
            &store,
            harness,
            &mut docker_cancel,
            cancelled,
        ),
        _ => unreachable!(),
    }
}

fn profile_for_platforms(platforms: &[QaPlatform]) -> Result<RuntimeProfile, String> {
    match platforms {
        [QaPlatform::Web | QaPlatform::HeadlessService] => Ok(RuntimeProfile::BrowserQa),
        [QaPlatform::LinuxDesktop] => Ok(RuntimeProfile::DesktopQa),
        _ => Err("runtime QA request has no single supported platform".into()),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_reviewed(
    request: &crate::runtime_qa::QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    authorization: QaTargetAuthorization<'_>,
    mut driver: impl crate::runtime_qa::QaScenarioDriver,
    harness: Box<dyn crate::harness::AiHarness>,
    workspace: &crate::execution::contract::WorkspaceRef,
    task_clone: &std::path::Path,
    store: &RuntimeQaStore,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer::manifest(request, scenarios, &reviewer_run, &reviewer_session)?;
    let mut reviewer_call =
        |evidence: &crate::runtime_qa::QaEvidenceManifestV1,
         candidate: &crate::runtime_qa::QaResultV1,
         screenshots: &[crate::runtime_qa::QaScreenshotArtifactV1]| {
            if cancelled.load(Ordering::Acquire) {
                return Err("runtime QA was cancelled before independent review".into());
            }
            reviewer::execute(reviewer::ReviewInputs {
                harness: harness.as_ref(),
                request,
                scenarios,
                evidence,
                candidate,
                screenshots,
                workspace,
                task_clone,
                reviewer_run: &reviewer_run,
                reviewer_session: &reviewer_session,
                cancelled: Arc::clone(&cancelled),
            })
        };
    let orchestrator = RuntimeQaOrchestrator::new(store);
    let (_result, _receipt) = orchestrator.execute_reviewed(
        crate::runtime_qa::QaExecutionContext {
            request,
            scenarios,
            authorization,
            driver: &mut driver,
            reviewer_execution_id: &reviewer_run,
            reviewer_session_id: &reviewer_session,
            prompt,
        },
        &mut reviewer_call,
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "run/tests.rs"]
mod tests;
