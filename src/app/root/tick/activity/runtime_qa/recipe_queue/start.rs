use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    app::session::Project,
    core::lifecycle::TaskRecord,
    execution::contract::ResourceBudgetV1,
    project_runtime::{
        ProjectRuntimeExecutor, ProjectRuntimeStore, RuntimeOperation, RuntimeReceiptState,
        RuntimeReceiptV1,
    },
};

use super::{candidate::now_unix_ms, completion, prerequisites};

pub(crate) fn start(
    project: &Project,
    task: &TaskRecord,
) -> Result<crate::runtime_qa::RuntimeRecipeWorkerHandle, String> {
    let candidate = super::candidate(project, task)?
        .ok_or_else(|| "no approved runtime recipe operation is ready".to_owned())?;
    let super::RecipeCandidate {
        target_id,
        operation,
        clone_path,
        workspace,
        required_tools,
        project_relative_root,
        required_dependencies,
        allowed_untracked_outputs,
    } = candidate;
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
    let recipe_refs: Vec<&str> = match operation {
        RuntimeOperation::Build => target.build_recipe_ref.iter().map(String::as_str).collect(),
        RuntimeOperation::Test => target.test_recipe_refs.iter().map(String::as_str).collect(),
        RuntimeOperation::Start => target.start_recipe_ref.iter().map(String::as_str).collect(),
    };
    let recipe_ids = recipe_refs
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let task_uid = task.task_uid.clone();
    let item_uid = task
        .planning_item_uid
        .as_deref()
        .ok_or("runtime recipe task has no planning item identity")?;
    let ticket = project
        .task_documents
        .iter()
        .find(|document| {
            document
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.uid == item_uid)
        })
        .map(|document| document.path.clone())
        .ok_or("runtime recipe task document disappeared")?;
    let selected_dependency_cache_refs =
        crate::app::root::runtime_dependencies::prepared_dependency_cache_refs_for_ticket(
            project,
            &ticket,
            &required_dependencies,
        )?;
    let mut claim_time = store
        .receipts_for_task(&workspace.task_id)
        .map_err(|error| error.to_string())?
        .iter()
        .map(|receipt| receipt.observed_at_unix_ms)
        .max()
        .unwrap_or_default()
        .saturating_add(1)
        .max(now_unix_ms());
    for recipe_id in &recipe_ids {
        if let Err(error) = store.append_receipt(&RuntimeReceiptV1 {
            schema_version: 1,
            receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            operation,
            task_id: workspace.task_id.clone(),
            target_id: target_id.clone(),
            source_sha: workspace.source_sha.clone(),
            recipe_id: Some(recipe_id.clone()),
            state: RuntimeReceiptState::Running,
            exit_code: None,
            artifact_hashes: Default::default(),
            prepared_dependency_ids: Vec::new(),
            log_reference: None,
            observed_at_unix_ms: claim_time,
        }) {
            let reason = format!("runtime recipe attempt could not be claimed: {error}");
            return match completion::fail_unfinished_claims(
                &planning_store,
                &workspace,
                &target_id,
                operation,
                &recipe_ids,
            ) {
                Ok(()) => Err(reason),
                Err(finalize_error) => Err(format!(
                    "{reason}; partial runtime recipe claims could not be finalized: {finalize_error}"
                )),
            };
        }
        claim_time = claim_time.saturating_add(1);
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let session_slot = Arc::new(Mutex::new(None));
    let worker_session_slot = Arc::clone(&session_slot);
    let (sender, result) = std::sync::mpsc::channel();
    let (progress_sender, progress) = std::sync::mpsc::channel();
    let spawn_store = planning_store.clone();
    let spawn_workspace = workspace.clone();
    let spawn_target_id = target_id.clone();
    let spawn_recipe_ids = recipe_ids.clone();
    let spawn_result = std::thread::Builder::new()
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
                let (session, _resource_bridge) = prerequisites::connect(
                    prerequisites::ConnectRequest {
                        workspace: workspace.clone(),
                        clone_path: &clone_path,
                        project_relative_root: &project_relative_root,
                        target_id: &target_id,
                        required_tools: &required_tools,
                        selected_dependency_cache_refs: &selected_dependency_cache_refs,
                        limits,
                        cancel: worker_cancel.clone(),
                        progress: progress_sender,
                        session_slot: Arc::clone(&worker_session_slot),
                    },
                )?;
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
            let outcome = match outcome {
                Ok(receipts) => Ok(receipts),
                Err(reason) => match completion::fail_unfinished_claims(
                    &planning_store,
                    &workspace,
                    &target_id,
                    operation,
                    &recipe_ids,
                ) {
                    Ok(()) => Err(reason),
                    Err(finalize_error) => Err(format!(
                        "{reason}; unfinished runtime recipe claims could not be finalized: {finalize_error}"
                    )),
                },
            };
            let _ = sender.send(outcome);
        });
    if let Err(error) = spawn_result {
        let reason = format!("runtime recipe worker could not start: {error}");
        return match completion::fail_unfinished_claims(
            &spawn_store,
            &spawn_workspace,
            &spawn_target_id,
            operation,
            &spawn_recipe_ids,
        ) {
            Ok(()) => Err(reason),
            Err(finalize_error) => Err(format!(
                "{reason}; runtime recipe claims could not be finalized: {finalize_error}"
            )),
        };
    }
    Ok(crate::runtime_qa::RuntimeRecipeWorkerHandle::new(
        task_uid,
        ticket,
        cancel,
        session_slot,
        progress,
        result,
    ))
}
