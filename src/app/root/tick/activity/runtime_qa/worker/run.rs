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
use crate::{
    execution::{
        contract::{ExecutionModeV1, ResourceBudgetV1},
        docker::{
            images::{ImageLock, RuntimeProfile},
            qa_container::{QaArtifactMount, snapshot_selected_files},
        },
        session::ExecutionSession,
    },
    runtime_qa::{
        QaPlatform, QaScenarioV1, QaTargetAuthorization, RuntimeQaOrchestrator, RuntimeQaStore,
    },
};

mod execution;

pub(super) fn run(
    prepared: PreparedRequest,
    scenarios: Vec<QaScenarioV1>,
    harness: Box<dyn crate::harness::AiHarness>,
    store_root: PathBuf,
    cancelled: Arc<AtomicBool>,
    mut docker_cancel: tokio::sync::watch::Receiver<bool>,
) -> Result<(), String> {
    let request = prepared.request.clone();
    let store = RuntimeQaStore::new(store_root);
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
        &prepared.workspace,
        &prepared.target,
        &prepared.startup_recipe,
        prepared.readiness_probe.as_ref(),
        &prepared.artifact_paths,
    )?;
    crate::project_runtime::verify_source_checkout(
        &prepared.task_clone,
        &request.code_head_sha,
        &prepared.artifact_paths,
    )?;
    let (artifact_upload, artifact_sha) = snapshot_selected_files(
        &prepared.task_clone,
        &prepared.artifact_paths,
        &prepared.build_receipt.artifact_hashes,
        request
            .bounded_limits
            .max_evidence_bytes
            .min(512 * 1024 * 1024) as usize,
    )
    .map_err(|error| error.message)?;
    if artifact_sha != request.build_artifact_sha256 {
        return Err(
            "immutable task artifact snapshot no longer matches the approved build receipt".into(),
        );
    }
    let profile = match request.relevant_platforms.as_slice() {
        [QaPlatform::Web] => RuntimeProfile::BrowserQa,
        [QaPlatform::LinuxDesktop] => RuntimeProfile::DesktopQa,
        _ => return Err("runtime QA request has no single supported platform".into()),
    };
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
    let lock = ImageLock::bundled().map_err(|error| error.message)?;
    let qa_image = lock
        .select(profile, &architecture)
        .map_err(|error| error.message)?;
    let artifact_mount = QaArtifactMount::inspect(&prepared.task_clone, "/workspace")
        .map_err(|error| error.message)?;
    match profile {
        RuntimeProfile::BrowserQa => execution::run_web(
            &session,
            &prepared,
            &request,
            &scenarios,
            &artifact_upload,
            artifact_mount,
            &resources,
            &qa_image,
            architecture,
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
            &store,
            harness,
            &mut docker_cancel,
            cancelled,
        ),
        _ => unreachable!(),
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
    allowed_untracked_outputs: &[String],
) -> Result<(), String> {
    crate::project_runtime::verify_source_checkout(
        task_clone,
        &request.code_head_sha,
        allowed_untracked_outputs,
    )?;
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer::manifest(request, &reviewer_run, &reviewer_session)?;
    let mut reviewer_call =
        |evidence: &crate::runtime_qa::QaEvidenceManifestV1,
         candidate: &crate::runtime_qa::QaResultV1| {
            if cancelled.load(Ordering::Acquire) {
                return Err("runtime QA was cancelled before independent review".into());
            }
            reviewer::execute(reviewer::ReviewInputs {
                harness: harness.as_ref(),
                request,
                evidence,
                candidate,
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
