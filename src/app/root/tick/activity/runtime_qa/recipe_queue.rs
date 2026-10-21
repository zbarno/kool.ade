use crate::{
    app::session::Project,
    core::lifecycle::{GateDecision, TaskPhase, TaskRecord},
    execution::contract::WorkspaceRef,
    project_runtime::ProjectRuntimeStore,
};

mod candidate;
use candidate::{ApprovedCandidate as RecipeCandidate, approved_candidate, next_operation};
mod start;
pub(crate) use start::start;
mod completion;
mod prerequisites;

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
            && candidate::supports_recipe_surface(target.app_surface)
            && candidate::can_queue_prerequisite_recovery(&target.capability_state)
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
            &candidate.allowed_untracked_outputs,
        )
        .is_err()
    }) {
        return Ok(None);
    }
    Ok(candidate)
}
