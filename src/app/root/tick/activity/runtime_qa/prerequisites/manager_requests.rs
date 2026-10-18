use crate::{
    app::session::Project,
    core::lifecycle::TaskRecord,
    execution::docker::images::ImageLock,
    harness::{
        DependencyDecision, DependencyFailureCategory, DependencyKind, DependencyNeed,
        DependencyRequest, DependencyRequestStatus, PackageEcosystem,
    },
    project_runtime::ProjectRuntimeStore,
    project_runtime_capability::{
        ApprovedCapabilityInventory, CapabilityGap, assess_manifest_target,
    },
};

use super::{available_broker_dependencies, locked_capabilities, task_progress_for_target};

/// Put missing, target-declared tools through the existing Man.ager dependency
/// review queue. The request records a tool identity only; broker execution is
/// not enabled for system tools, so it cannot become an arbitrary install.
pub fn queue_missing_toolchain_reviews(
    project: &mut Project,
    task: &TaskRecord,
    request: &crate::runtime_qa::QaRunRequestV1,
) -> Vec<String> {
    let Some(planning_item_uid) = task.planning_item_uid.as_deref() else {
        return Vec::new();
    };
    let runtime = ProjectRuntimeStore::new(&project.state.planning_store);
    let Ok(Some(manifest)) = runtime.load_manifest() else {
        return Vec::new();
    };
    if manifest.source_sha != request.code_head_sha || request.parent_task_uid != task.task_uid {
        return Vec::new();
    }
    let Some(target) = manifest.candidate_targets.iter().find(|target| {
        target.owning_task_id == planning_item_uid
            && target.start_recipe_ref.as_deref() == Some(request.startup_recipe_ref.as_str())
            && manifest.is_active_target(planning_item_uid, &target.target_id)
    }) else {
        return Vec::new();
    };
    let Ok(lock) = ImageLock::bundled() else {
        return Vec::new();
    };
    let (tools, drivers) = locked_capabilities(&lock);
    let task_progress =
        task_progress_for_target(&project.task_documents, &project.activity.tasks, target);
    let cache_store = crate::execution::DependencyCacheSnapshotStore::new(
        crate::persistence::state_root().join("runtime-qa-dependency-caches"),
    );
    let dependencies = available_broker_dependencies(
        target,
        task,
        &project
            .state
            .planning_store
            .project_id
            .hyphenated()
            .to_string(),
        task_progress,
        &cache_store,
    );
    let inventory = ApprovedCapabilityInventory {
        manifest: &manifest,
        available_tool_ids: &tools,
        available_dependency_ids: &dependencies,
        available_driver_ids: &drivers,
    };
    let Ok(assessment) =
        assess_manifest_target(&manifest, &target.target_id, planning_item_uid, &inventory)
    else {
        return Vec::new();
    };
    let missing_tools = assessment
        .report
        .gaps
        .iter()
        .filter_map(|gap| match gap {
            CapabilityGap::MissingToolchain(id) => Some(id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if missing_tools.is_empty() {
        return missing_tools;
    }

    let recipe_ids = target
        .build_recipe_ref
        .iter()
        .chain(target.test_recipe_refs.iter())
        .chain(target.start_recipe_ref.iter())
        .cloned()
        .collect::<Vec<_>>();
    queue_missing_recipe_toolchain_reviews(
        project,
        task,
        &target.target_id,
        &manifest.workspace_id,
        &manifest.source_sha,
        &recipe_ids,
        &missing_tools,
    )
}

/// Route missing prerequisites for an exact approved Build/Test/Start recipe
/// through Man.ager without inventing an installation command.
pub fn queue_missing_recipe_toolchain_reviews(
    project: &mut Project,
    task: &TaskRecord,
    target_id: &str,
    workspace_id: &str,
    source_sha: &str,
    recipe_ids: &[String],
    missing_tools: &[String],
) -> Vec<String> {
    let Some(planning_item_uid) = task.planning_item_uid.as_deref() else {
        return Vec::new();
    };
    if missing_tools.is_empty() {
        return Vec::new();
    }
    let runtime = ProjectRuntimeStore::new(&project.state.planning_store);
    let Ok(Some(manifest)) = runtime.load_manifest() else {
        return Vec::new();
    };
    if manifest.workspace_id != workspace_id || manifest.source_sha != source_sha {
        return Vec::new();
    }
    let Some(target) = manifest.candidate_targets.iter().find(|target| {
        target.target_id == target_id
            && target.owning_task_id == planning_item_uid
            && manifest.is_active_target(planning_item_uid, target_id)
    }) else {
        return Vec::new();
    };
    let selected_recipes = recipe_ids
        .iter()
        .filter_map(|id| {
            manifest
                .recipes
                .iter()
                .chain(manifest.user_overrides.iter())
                .find(|recipe| recipe.recipe_id == *id)
        })
        .collect::<Vec<_>>();
    if selected_recipes.len() != recipe_ids.len()
        || selected_recipes
            .iter()
            .any(|recipe| !runtime.recipe_is_locally_approved(&manifest, target, recipe))
        || missing_tools.iter().any(|tool| {
            !selected_recipes.iter().any(|recipe| {
                recipe.executable_id.as_str() == tool.as_str()
                    || recipe.prerequisite_tool_ids.contains(tool)
            })
        })
    {
        return Vec::new();
    }
    let Some(document) = project.task_documents.iter().find(|document| {
        document
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.uid == target.owning_task_id)
    }) else {
        return Vec::new();
    };
    let ticket = document.path.clone();
    let mut scheduled = Vec::new();
    let progress = project.activity.tasks.entry(ticket.clone()).or_default();
    for tool_id in missing_tools {
        let source = format!(
            "runtime-capability://{}/{}@{}",
            manifest.workspace_id, target.target_id, manifest.source_sha
        );
        let existing = progress
            .dependency_requests
            .iter()
            .find(|existing| {
                existing.need.ecosystem == PackageEcosystem::System
                    && existing.need.kind == DependencyKind::SystemTool
                    && existing.need.package.as_deref() == Some(tool_id.as_str())
                    && existing.need.source.as_deref() == Some(source.as_str())
            })
            .cloned();
        if existing.is_some() {
            continue;
        }
        let dependency = toolchain_request(
            &task.task_uid,
            planning_item_uid,
            &target.target_id,
            &manifest.workspace_id,
            &manifest.source_sha,
            tool_id,
        );
        progress.dependency_requests.push(dependency.clone());
        project
            .activity
            .pending_dependency_reviews
            .push((ticket.clone(), dependency));
        scheduled.push(tool_id.clone());
    }
    if !scheduled.is_empty() {
        project.save_task_activity(&ticket);
        project.activity.mark_ticket_dirty(&ticket);
    }
    scheduled
}

fn request_id(
    task_id: &str,
    target_id: &str,
    workspace_id: &str,
    source_sha: &str,
    tool_id: &str,
) -> String {
    let namespace = uuid::Uuid::parse_str(task_id).unwrap_or(uuid::Uuid::NAMESPACE_OID);
    let name =
        format!("project-runtime-toolchain-v1:{workspace_id}:{target_id}:{source_sha}:{tool_id}");
    format!(
        "dependency-{}",
        uuid::Uuid::new_v5(&namespace, name.as_bytes()).hyphenated()
    )
}

fn toolchain_request(
    task_id: &str,
    planning_item_uid: &str,
    target_id: &str,
    workspace_id: &str,
    source_sha: &str,
    tool_id: &str,
) -> DependencyRequest {
    DependencyRequest {
        id: request_id(task_id, target_id, workspace_id, source_sha, tool_id),
        task_id: planning_item_uid.to_owned(),
        need: DependencyNeed {
            ecosystem: PackageEcosystem::System,
            package: Some(tool_id.to_owned()),
            version: None,
            source: Some(format!(
                "runtime-capability://{workspace_id}/{target_id}@{source_sha}"
            )),
            command: "approved runtime toolchain prerequisite".into(),
            reason: format!(
                "The selected project runtime target `{target_id}` at source `{source_sha}` declares toolchain `{tool_id}`, which is absent from the approved execution image."
            ),
            kind: DependencyKind::SystemTool,
            lockfile_identity: None,
            introduced_packages: Vec::new(),
        },
        category: DependencyFailureCategory::DependencySystemPackageRequired,
        decision: DependencyDecision::RequiresUserAuthorization,
        rationale: "Man.ager review is queued to classify this missing project runtime toolchain prerequisite.".into(),
        risk: "No installation or host command is authorized; a reviewed execution image or broker adapter is required.".into(),
        status: DependencyRequestStatus::Pending,
        preparation: None,
    }
}

#[cfg(test)]
mod tests;
