use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    app::session::Project,
    core::lifecycle::{GateDecision, TaskPhase, TaskRecord},
    execution::{
        contract::{ExecutionModeV1, ResourceBudgetV1, WorkspaceRef},
        session::ExecutionSession,
    },
    project_runtime::{
        AppSurface, ProjectRuntimeExecutor, ProjectRuntimeStore, RuntimeOperation,
        RuntimeReceiptState, RuntimeReceiptV1, TargetCapabilityState,
    },
};

mod candidate;
use candidate::{approved_candidate, next_operation, now_unix_ms};

type RecipeCandidate = (
    String,
    RuntimeOperation,
    PathBuf,
    WorkspaceRef,
    Vec<String>,
    Vec<String>,
);

pub(crate) fn start(
    project: &Project,
    task: &TaskRecord,
) -> Result<crate::runtime_qa::RuntimeRecipeWorkerHandle, String> {
    let (target_id, operation, clone_path, workspace, required_tools, allowed_untracked_outputs) =
        candidate(project, task)?
            .ok_or_else(|| "no approved runtime recipe operation is ready".to_owned())?;
    let planning_store = project.state.planning_store.clone();
    let store = ProjectRuntimeStore::new(&planning_store);
    let manifest = store
        .load_manifest()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "runtime manifest disappeared before recipe claim".to_owned())?;
    let target = manifest
        .candidate_targets
        .iter()
        .find(|target| target.target_id == target_id)
        .ok_or_else(|| "runtime target disappeared before recipe claim".to_owned())?;
    let recipe_ids: Vec<&str> = match operation {
        RuntimeOperation::Build => target.build_recipe_ref.iter().map(String::as_str).collect(),
        RuntimeOperation::Test => target.test_recipe_refs.iter().map(String::as_str).collect(),
        RuntimeOperation::Start => target.start_recipe_ref.iter().map(String::as_str).collect(),
    };
    let mut claim_time = store
        .receipts_for_task(&workspace.task_id)
        .map_err(|error| error.to_string())?
        .iter()
        .map(|receipt| receipt.observed_at_unix_ms)
        .max()
        .unwrap_or_default()
        .saturating_add(1)
        .max(now_unix_ms());
    for recipe_id in recipe_ids {
        store
            .append_receipt(&RuntimeReceiptV1 {
                schema_version: 1,
                receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
                operation,
                task_id: workspace.task_id.clone(),
                target_id: target_id.clone(),
                source_sha: workspace.source_sha.clone(),
                recipe_id: Some(recipe_id.to_owned()),
                state: RuntimeReceiptState::Running,
                exit_code: None,
                artifact_hashes: Default::default(),
                log_reference: None,
                observed_at_unix_ms: claim_time,
            })
            .map_err(|error| format!("runtime recipe attempt could not be claimed: {error}"))?;
        claim_time = claim_time.saturating_add(1);
    }
    let task_uid = task.task_uid.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let session_slot = Arc::new(Mutex::new(None));
    let worker_session_slot = Arc::clone(&session_slot);
    let (sender, result) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(format!("runtime-recipe-{task_uid}"))
        .spawn(move || {
            let outcome = (|| {
                if worker_cancel.load(Ordering::Acquire) {
                    return Err("runtime recipe was cancelled before session creation".into());
                }
                let limits = ResourceBudgetV1 {
                    cpu_millis: 2_000,
                    memory_bytes: 4 * 1024 * 1024 * 1024,
                    process_count: 128,
                    temporary_disk_bytes: 2 * 1024 * 1024 * 1024,
                    output_bytes: 64 * 1024 * 1024,
                };
                let expected_source_sha = workspace.source_sha.clone();
                crate::project_runtime::verify_source_checkout(
                    &clone_path,
                    &workspace.source_sha,
                    &allowed_untracked_outputs,
                )?;
                let session = Arc::new(
                    ExecutionSession::connect_local_with_tools(
                        workspace,
                        &clone_path,
                        ExecutionModeV1::Verification,
                        &required_tools,
                        limits,
                    )
                    .map_err(|error| {
                        format!("isolated recipe executor unavailable: {}", error.message)
                    })?,
                );
                *worker_session_slot
                    .lock()
                    .map_err(|_| "recipe worker cancellation state is unavailable".to_owned())? =
                    Some(Arc::clone(&session));
                if worker_cancel.load(Ordering::Acquire) {
                    let _ = session.cancel();
                    return Err("runtime recipe was cancelled before execution".into());
                }
                crate::project_runtime::verify_source_checkout(
                    &clone_path,
                    &expected_source_sha,
                    &allowed_untracked_outputs,
                )?;
                let store = ProjectRuntimeStore::new(&planning_store);
                let executor = ProjectRuntimeExecutor::new(&store);
                let receipts = executor
                    .execute(&session, &target_id, operation)
                    .map_err(|error| format!("runtime recipe execution failed: {error}"))?;
                if worker_cancel.load(Ordering::Acquire) {
                    return Err("runtime recipe was cancelled".into());
                }
                if receipts.is_empty()
                    || receipts
                        .iter()
                        .any(|receipt| receipt.state != RuntimeReceiptState::Passed)
                {
                    return Err(format!("approved {operation:?} recipe did not pass"));
                }
                Ok(receipts)
            })();
            let _ = sender.send(outcome);
        })
        .map_err(|error| format!("runtime recipe worker could not start: {error}"))?;
    Ok(crate::runtime_qa::RuntimeRecipeWorkerHandle::new(
        task_uid,
        cancel,
        session_slot,
        result,
    ))
}

pub(crate) fn ready(project: &Project, task: &TaskRecord) -> Result<bool, String> {
    candidate(project, task).map(|candidate| candidate.is_some())
}

fn candidate(project: &Project, task: &TaskRecord) -> Result<Option<RecipeCandidate>, String> {
    if task.phase != TaskPhase::AwaitingRuntimeQa {
        return Ok(None);
    }
    let Some(item_uid) = task.planning_item_uid.as_deref() else {
        return Ok(None);
    };
    let Some(document) = project.task_documents.iter().find(|document| {
        document
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.uid == item_uid)
    }) else {
        return Ok(None);
    };
    let Some(implementation) = project.implementation_states.get(&document.path) else {
        return Ok(None);
    };
    let Some(source_sha) = implementation.verified_head.as_deref() else {
        return Ok(None);
    };
    if task.reviewed_code_head_sha.as_deref() != Some(source_sha)
        || task.code_review.as_ref().is_none_or(|review| {
            review.decision != GateDecision::Approved
                || review.subject_uid != task.task_uid
                || task.implementation_sha256.as_deref() != Some(review.subject_sha256.as_str())
                || review.author_run_id.is_none()
                || review.author_session_id.is_none()
                || review.reviewer_run_id.is_none()
                || review.reviewer_session_id.is_none()
        })
    {
        return Ok(None);
    }
    let workspace = WorkspaceRef {
        project_id: implementation
            .workspace_uid
            .clone()
            .ok_or("task clone has no workspace identity")?,
        task_id: item_uid.to_owned(),
        source_sha: source_sha.to_owned(),
    };
    workspace
        .validate()
        .map_err(|_| "task clone workspace identity is invalid")?;
    let store = ProjectRuntimeStore::new(&project.state.planning_store);
    let manifest = store.load_manifest().map_err(|error| error.to_string())?;
    let Some(manifest) = manifest else {
        return Ok(None);
    };
    if manifest.workspace_id != workspace.project_id || manifest.source_sha != source_sha {
        return Ok(None);
    }
    let Some(target) = manifest.candidate_targets.iter().find(|target| {
        target.owning_task_id == item_uid
            && target.repository_id == manifest.repository_id
            && manifest.is_active_target(item_uid, &target.target_id)
            && matches!(
                target.app_surface,
                AppSurface::Web | AppSurface::LinuxDesktop
            )
            && matches!(
                target.capability_state,
                TargetCapabilityState::Ready | TargetCapabilityState::ReadyButUntested
            )
    }) else {
        return Ok(None);
    };
    let receipts = store
        .receipts_for_task(item_uid)
        .map_err(|error| error.to_string())?;
    let Some((operation, recipe_ids)) = next_operation(target, &receipts, source_sha)? else {
        return Ok(None);
    };
    for recipe_id in &recipe_ids {
        let recipe = manifest
            .recipes
            .iter()
            .chain(manifest.user_overrides.iter())
            .find(|recipe| recipe.recipe_id == *recipe_id)
            .ok_or("approved target recipe is missing")?;
        if !store.recipe_is_locally_approved(&manifest, target, recipe) {
            return Ok(None);
        }
    }
    let candidate = approved_candidate(
        &manifest,
        target,
        &implementation.task_repository,
        workspace,
        operation,
        recipe_ids.iter().map(String::as_str).collect(),
    )?;
    if candidate.as_ref().is_some_and(|candidate| {
        crate::project_runtime::verify_source_checkout(
            &implementation.task_repository,
            source_sha,
            &candidate.5,
        )
        .is_err()
    }) {
        return Ok(None);
    }
    Ok(candidate)
}
