use crate::{
    app::session::Project,
    core::lifecycle::{GateActor, GateDecision, GateReceipt},
    project_runtime::{
        AppSurface, AppTargetV1, ProjectRuntimeStore, ReadinessProbeV1, TargetCapabilityState,
    },
    ui::task_detail::RuntimeRecipeInput,
};

mod recipe;
use recipe::append_recipe;

#[cfg(test)]
#[path = "target_override/tests.rs"]
mod tests;

#[allow(clippy::too_many_arguments)]
pub(super) fn add(
    project: &Project,
    ticket: &str,
    app_surface: AppSurface,
    project_relative_root: &str,
    fixture_ref: &str,
    artifact_paths: &str,
    build: &RuntimeRecipeInput,
    test: &RuntimeRecipeInput,
    start: &RuntimeRecipeInput,
    required_dependencies: &[String],
    approval_proof: crate::ui::layout::RuntimeTargetApprovalProof,
    readiness_port: &str,
    readiness_path: &str,
    expected_revision: u64,
) -> Result<String, String> {
    if let Err(message) =
        crate::project_runtime::validate_custom_target_root(project_relative_root.trim())
    {
        return Err(message.into());
    }
    let artifact_paths = artifact_paths
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let has_build = !build.executable_id.trim().is_empty();
    if crate::project_runtime::validate_custom_target_artifact_paths(has_build, &artifact_paths)
        .is_err()
    {
        return Err(
            "Build recipes require 1 to 128 unique canonical repository-relative artifact paths; Test-only targets must leave the artifact list empty.".into(),
        );
    }
    let document = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
        .ok_or_else(|| "Task is no longer available.".to_owned())?;
    let metadata = document
        .metadata
        .as_ref()
        .ok_or_else(|| "Task identity is unavailable.".to_owned())?;
    let available_dependencies =
        crate::app::root::runtime_dependencies::prepared_dependencies_for_ticket(project, ticket)
            .into_iter()
            .map(|dependency| dependency.request_id)
            .collect::<std::collections::BTreeSet<_>>();
    let selected_dependencies =
        validate_required_dependencies(required_dependencies, &available_dependencies)?;
    let implementation = project
        .implementation_states
        .get(ticket)
        .ok_or_else(|| "Task implementation is unavailable.".to_owned())?;
    let workspace_id = implementation
        .workspace_uid
        .as_deref()
        .ok_or_else(|| "Task workspace identity is unavailable.".to_owned())?;
    let source_sha = implementation
        .verified_head
        .as_deref()
        .ok_or_else(|| "Task source has no verified commit yet.".to_owned())?;
    let store = ProjectRuntimeStore::new(&project.state.planning_store);
    let manifest = store
        .load_manifest()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Discover project targets before adding a custom target.".to_owned())?;
    if implementation
        .task_uid
        .as_deref()
        .is_some_and(|uid| uid != metadata.uid)
        || manifest.workspace_id != workspace_id
        || manifest.repository_id != metadata.repository_id
        || manifest.source_sha != source_sha
    {
        return Err("Runtime state is stale or belongs to a different task, workspace, repository, or source revision. Discover again.".into());
    }

    let approval_id = approval_proof.receipt_uid();
    let approval_ref = format!("user-approval:{approval_id}");
    let mut recipes = Vec::new();
    let build_ref = append_recipe(
        &mut recipes,
        "build",
        app_surface,
        build,
        &approval_ref,
        &artifact_paths,
    )?;
    let test_ref = append_recipe(&mut recipes, "test", app_surface, test, &approval_ref, &[])?;
    let start_ref = append_recipe(
        &mut recipes,
        "start",
        app_surface,
        start,
        &approval_ref,
        &[],
    )?;
    if recipes.is_empty() {
        return Err("Add at least one complete build, test, or start recipe.".into());
    }
    if start_ref.is_some() && build_ref.is_none() {
        return Err(
            "A Start recipe requires a Build recipe so its outputs can be verified.".into(),
        );
    }
    for recipe in &mut recipes {
        recipe.evidence_refs.push(format!(
            "user-recipe-approval:{}",
            uuid::Uuid::new_v4().hyphenated()
        ));
    }
    let readiness = if start_ref.is_some() {
        Some(match app_surface {
            AppSurface::Web => ReadinessProbeV1::HttpOnApprovedPort {
                port: parse_port(readiness_port, "web")?,
                path: readiness_path.trim().to_owned(),
                expected_status: 200,
            },
            AppSurface::HeadlessService => ReadinessProbeV1::LocalServiceHealth {
                port: parse_port(readiness_port, "service")?,
                path: readiness_path.trim().to_owned(),
            },
            AppSurface::LinuxDesktop => ReadinessProbeV1::ProcessAlive {
                executable_id: start.executable_id.trim().to_owned(),
            },
            AppSurface::UnsupportedTarget => {
                return Err("Unsupported targets cannot be approved for runtime QA.".into());
            }
        })
    } else {
        None
    };
    let target_id = format!("user-target-{}", uuid::Uuid::new_v4().simple());
    let artifact_spec_id = build_ref
        .as_ref()
        .map(|_| format!("user-artifacts-{}", uuid::Uuid::new_v4().simple()));
    let artifact_spec = artifact_spec_id.as_ref().map(|_| &artifact_paths);
    let target = AppTargetV1 {
        target_id,
        owning_task_id: metadata.uid.clone(),
        repository_id: metadata.repository_id.clone(),
        app_surface,
        project_relative_root: crate::project_runtime::normalize_custom_target_root(
            project_relative_root.trim(),
        )?,
        build_recipe_ref: build_ref,
        test_recipe_refs: test_ref.into_iter().collect(),
        start_recipe_ref: start_ref,
        readiness_probe_ref: None,
        artifact_spec_ref: artifact_spec_id,
        fixture_ref: (!fixture_ref.trim().is_empty()).then(|| fixture_ref.trim().to_owned()),
        required_dependencies: selected_dependencies.into_iter().collect(),
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default-deny".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
    target
        .validate()
        .map_err(|error| format!("Custom target is invalid: {error}"))?;
    let approval = GateReceipt {
        receipt_uid: approval_id,
        subject_uid: metadata.uid.clone(),
        subject_sha256: ProjectRuntimeStore::user_target_proposal_sha256(
            &manifest.workspace_id,
            &manifest.repository_id,
            &manifest.source_sha,
            &target,
            &recipes,
            readiness.as_ref(),
            artifact_spec,
        )?,
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: now_unix_ms(),
        evidence_refs: vec![format!("runtime-target:{}", target.target_id)],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    store
        .add_user_target_with_readiness_and_save(
            target,
            recipes,
            readiness,
            artifact_paths,
            approval,
            approval_proof,
            expected_revision,
        )
        .map(|_| {
            "Custom target and approved recipes saved. QA remains untested until execution."
                .to_owned()
        })
        .map_err(|error| error.to_string())
}

fn validate_required_dependencies(
    required: &[String],
    available: &std::collections::BTreeSet<String>,
) -> Result<Vec<String>, String> {
    let prepared = available.iter().cloned().collect::<Vec<_>>();
    crate::app::root::runtime_dependencies::validate_selected_dependency_ids(required, &prepared)
        .map_err(|_| {
            "Custom target dependencies must be selected from this task's intact, successfully prepared broker inventory."
                .into()
        })
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn parse_port(value: &str, kind: &str) -> Result<u16, String> {
    let port = value
        .trim()
        .parse::<u16>()
        .map_err(|_| format!("Enter a valid approved {kind} port."))?;
    if port == 0 {
        return Err(format!("Enter a valid approved {kind} port."));
    }
    Ok(port)
}
