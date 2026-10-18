use crate::{
    app::session::Project,
    core::lifecycle::{GateActor, GateDecision, GateReceipt},
    project_runtime::{
        AppSurface, AppTargetV1, ExecutionRecipeV1, NetworkRequirement, ProjectRuntimeStore,
        ReadinessProbeV1, RecipeSource, TargetCapabilityState,
    },
    ui::task_detail::RuntimeRecipeInput,
};

#[cfg(test)]
#[path = "target_override/tests.rs"]
mod tests;

#[allow(clippy::too_many_arguments)]
pub(super) fn add(
    project: &Project,
    ticket: &str,
    app_surface: AppSurface,
    project_relative_root: &str,
    artifact_paths: &str,
    required_dependency_ids: &str,
    build: &RuntimeRecipeInput,
    test: &RuntimeRecipeInput,
    start: &RuntimeRecipeInput,
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
    if crate::project_runtime::validate_custom_artifact_paths(&artifact_paths).is_err() {
        return Err(
            "Add 1 to 128 unique canonical repository-relative build artifact paths.".into(),
        );
    }
    let required_dependencies = parse_dependency_ids(required_dependency_ids)?;
    let document = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
        .ok_or_else(|| "Task is no longer available.".to_owned())?;
    let metadata = document
        .metadata
        .as_ref()
        .ok_or_else(|| "Task identity is unavailable.".to_owned())?;
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
    let build_ref = append_recipe(&mut recipes, "build", build, &approval_ref, &artifact_paths)?;
    let test_ref = append_recipe(&mut recipes, "test", test, &approval_ref, &[])?;
    let start_ref = append_recipe(&mut recipes, "start", start, &approval_ref, &[])?;
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
    let artifact_spec_id = format!("user-artifacts-{}", uuid::Uuid::new_v4().simple());
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
        artifact_spec_ref: Some(artifact_spec_id),
        fixture_ref: None,
        required_dependencies,
        supported_driver_ids: Vec::new(),
        port_needs: Vec::new(),
        environment_policy_ref: "default-deny".into(),
        capability_state: TargetCapabilityState::ReadyButUntested,
    };
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
            Some(&artifact_paths),
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

fn parse_dependency_ids(input: &str) -> Result<Vec<String>, String> {
    let ids = input
        .lines()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if ids.len() > 128 {
        return Err("A target can reference at most 128 dependency broker requests.".into());
    }
    let unique = ids.iter().collect::<std::collections::BTreeSet<_>>();
    if unique.len() != ids.len() {
        return Err("Dependency broker request IDs must be unique.".into());
    }
    if ids.iter().any(|id| {
        id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    }) {
        return Err("Dependency broker request IDs must use letters, numbers, dots, dashes, or underscores.".into());
    }
    Ok(ids)
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn append_recipe(
    recipes: &mut Vec<ExecutionRecipeV1>,
    operation: &str,
    input: &RuntimeRecipeInput,
    approval_ref: &str,
    artifact_paths: &[String],
) -> Result<Option<String>, String> {
    let executable_id = input.executable_id.trim();
    let argv = input
        .argv_lines
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if executable_id.is_empty() && argv.is_empty() {
        return Ok(None);
    }
    if executable_id.is_empty() || argv.is_empty() {
        return Err(format!(
            "The {operation} recipe needs both an executable ID and arguments."
        ));
    }
    if argv.len() > 128 || argv.iter().any(|arg| arg.chars().any(char::is_control)) {
        return Err(format!(
            "The {operation} recipe has control characters or too many arguments."
        ));
    }
    let recipe_id = format!("user-{}-{}", operation, uuid::Uuid::new_v4().simple());
    recipes.push(ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: recipe_id.clone(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: executable_id.to_owned(),
        argv,
        allowed_env_keys: Vec::new(),
        allowed_output_paths: if operation == "build" {
            artifact_paths.to_vec()
        } else {
            Vec::new()
        },
        timeout_seconds: 1200,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: vec![executable_id.to_owned()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![approval_ref.to_owned()],
    });
    Ok(Some(recipe_id))
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
