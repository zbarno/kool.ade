use crate::{
    app::session::Project,
    core::{implementation::Implementation, lifecycle::TaskRecord},
    execution::docker::images::{ImageLock, RuntimeProfile},
    project_runtime::{
        AppSurface, AppTargetV1, ProjectRuntimeStore, RecipeSource, RuntimeOperation,
        RuntimeReceiptState, RuntimeReceiptV1,
    },
    runtime_qa::QaRunRequestV1,
};
mod artifact;
mod task_spec;
mod validated;
use artifact::recipe_sha256;
use validated::from_reviewed_task;

/// Build from reviewed lifecycle state, persisted task text, a verified head,
/// approved target, successful build receipt, and exact artifact hashes.
pub(super) struct ReviewedTaskInputs<'a> {
    pub task: &'a TaskRecord,
    pub task_text: &'a str,
    pub implementation: &'a Implementation,
    pub target: &'a AppTargetV1,
    pub startup_recipe_sha256: &'a str,
    pub build: &'a RuntimeReceiptV1,
    pub artifact_paths: &'a [String],
    pub driver_profile_version: &'a str,
}

pub(super) struct PreparedRequest {
    pub request: QaRunRequestV1,
    pub task_clone: std::path::PathBuf,
    pub workspace: crate::execution::contract::WorkspaceRef,
    pub target: AppTargetV1,
    pub startup_recipe: crate::project_runtime::ExecutionRecipeV1,
    pub build_receipt: RuntimeReceiptV1,
    pub artifact_paths: Vec<String>,
    pub readiness_probe: Option<crate::project_runtime::ReadinessProbeV1>,
    pub task_routes:
        std::collections::BTreeMap<String, crate::persistence::harness_settings::WorkRoute>,
}

/// Resolve request inputs only from persisted task, review, runtime target,
/// build receipt, selected artifact snapshot, and explicit scenario approval.
pub(super) fn prepare(project: &Project, task: &TaskRecord) -> Result<PreparedRequest, String> {
    let planning_item_uid = task
        .planning_item_uid
        .as_deref()
        .ok_or_else(|| "lifecycle QA task is not bound to a planning item".to_owned())?;
    let document = project
        .task_documents
        .iter()
        .find(|document| {
            document
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.uid == planning_item_uid)
        })
        .ok_or_else(|| "the exact persisted task document is unavailable".to_owned())?;
    let implementation = project
        .implementation_states
        .get(&document.path)
        .ok_or_else(|| "the verified task clone is unavailable".to_owned())?;
    let manifest = ProjectRuntimeStore::new(&project.state.planning_store)
        .load_manifest()
        .map_err(|error| format!("project runtime manifest could not be validated: {error}"))?
        .ok_or_else(|| "the project has no approved runtime capability manifest".to_owned())?;
    let target = manifest
        .candidate_targets
        .iter()
        .find(|target| {
            target.owning_task_id == planning_item_uid
                && matches!(
                    target.app_surface,
                    AppSurface::Web | AppSurface::LinuxDesktop
                )
        })
        .ok_or_else(|| "the manifest has no supported target for this exact task".to_owned())?;
    let recipe = manifest
        .recipes
        .iter()
        .chain(manifest.user_overrides.iter())
        .find(|recipe| Some(recipe.recipe_id.as_str()) == target.start_recipe_ref.as_deref())
        .ok_or_else(|| "the approved target startup recipe is missing".to_owned())?;
    if !matches!(
        recipe.source,
        RecipeSource::UserApproved | RecipeSource::TrustedAdapter
    ) || (recipe.source == RecipeSource::UserApproved
        && !recipe
            .evidence_refs
            .iter()
            .any(|value| value.starts_with("user-approval:")))
    {
        return Err(
            "the target startup recipe has no explicit user approval or trusted adapter".into(),
        );
    }
    let runtime = ProjectRuntimeStore::new(&project.state.planning_store);
    let build_recipe = manifest
        .recipes
        .iter()
        .chain(manifest.user_overrides.iter())
        .find(|recipe| Some(recipe.recipe_id.as_str()) == target.build_recipe_ref.as_deref())
        .ok_or_else(|| "the approved target build recipe is missing".to_owned())?;
    if !matches!(
        build_recipe.source,
        RecipeSource::UserApproved | RecipeSource::TrustedAdapter
    ) || (build_recipe.source == RecipeSource::UserApproved
        && !build_recipe
            .evidence_refs
            .iter()
            .any(|value| value.starts_with("user-approval:")))
    {
        return Err(
            "the target build recipe has no explicit user approval or trusted adapter".into(),
        );
    }
    let build_receipts = runtime
        .receipts_for_task(planning_item_uid)
        .map_err(|error| format!("task runtime receipts could not be validated: {error}"))?;
    let build = latest_successful_build_receipt(
        &build_receipts,
        &target.target_id,
        implementation.verified_head.as_deref().unwrap_or_default(),
        target.build_recipe_ref.as_deref().unwrap_or_default(),
    )
    .ok_or_else(|| {
        "no successful build receipt exists for the verified task head and target".to_owned()
    })?;
    let artifact_ref = target
        .artifact_spec_ref
        .as_deref()
        .ok_or_else(|| "the approved target has no artifact allowlist".to_owned())?;
    let artifact_paths = manifest
        .artifact_specs
        .get(artifact_ref)
        .ok_or_else(|| "the target artifact allowlist is missing".to_owned())?;
    let profile = match target.app_surface {
        AppSurface::Web => RuntimeProfile::BrowserQa,
        AppSurface::LinuxDesktop => RuntimeProfile::DesktopQa,
        _ => return Err("the target platform has no supported QA profile".into()),
    };
    let profile_key = match profile {
        RuntimeProfile::BrowserQa => "browser_qa",
        RuntimeProfile::DesktopQa => "desktop_qa",
        _ => return Err("the selected image profile is not a QA profile".into()),
    };
    let lock = ImageLock::bundled().map_err(|error| error.message)?;
    let driver_version = lock
        .profiles
        .get(profile_key)
        .and_then(|images| images.first())
        .and_then(|image| image.tool_versions.get("driver"))
        .ok_or({
            format!(
                "the approved {profile_key} image profile has no reviewed driver version; build and review the pinned QA image, then retry"
            )
        })?;
    let request = from_reviewed_task(ReviewedTaskInputs {
        task,
        task_text: &document.text,
        implementation,
        target,
        startup_recipe_sha256: &recipe_sha256(recipe)?,
        build,
        artifact_paths,
        driver_profile_version: driver_version,
    })?;
    let readiness_probe = target
        .readiness_probe_ref
        .as_ref()
        .and_then(|reference| manifest.readiness_probes.get(reference))
        .cloned();
    let workspace = crate::execution::contract::WorkspaceRef {
        project_id: implementation
            .workspace_uid
            .clone()
            .ok_or_else(|| "runtime QA task clone has no workspace identity".to_owned())?,
        task_id: task.task_uid.clone(),
        source_sha: request.code_head_sha.clone(),
    };
    workspace
        .validate()
        .map_err(|_| "runtime QA workspace identity is invalid".to_owned())?;
    Ok(PreparedRequest {
        request,
        task_clone: implementation.task_repository.clone(),
        workspace,
        target: target.clone(),
        startup_recipe: recipe.clone(),
        build_receipt: build.clone(),
        artifact_paths: artifact_paths.clone(),
        readiness_probe,
        task_routes: document
            .metadata
            .as_ref()
            .map(|metadata| metadata.routing_overrides.clone())
            .unwrap_or_default(),
    })
}

pub(super) fn latest_successful_build_receipt<'a>(
    receipts: &'a [RuntimeReceiptV1],
    target_id: &str,
    source_sha: &str,
    recipe_id: &str,
) -> Option<&'a RuntimeReceiptV1> {
    receipts
        .iter()
        .filter(|receipt| {
            receipt.operation == RuntimeOperation::Build
                && receipt.state == RuntimeReceiptState::Passed
                && receipt.target_id == target_id
                && receipt.source_sha == source_sha
                && receipt.recipe_id.as_deref() == Some(recipe_id)
        })
        .max_by_key(|receipt| (receipt.observed_at_unix_ms, receipt.receipt_id.clone()))
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
