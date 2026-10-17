use super::{KooladeApp, Screen};
use crate::{
    project_runtime::{
        AppSurface, DiscoveryContext, ExecutionRecipeV1, NetworkRequirement, ProjectRuntimeStore,
        ReadinessProbeV1, RecipeSource, RuntimeOperation,
    },
    ui::task_detail::RuntimeRecipeOperation,
};

mod target_override;

impl KooladeApp {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn add_runtime_target(
        &mut self,
        ticket: &str,
        app_surface: AppSurface,
        project_relative_root: &str,
        artifact_paths: &str,
        build: &crate::ui::task_detail::RuntimeRecipeInput,
        test: &crate::ui::task_detail::RuntimeRecipeInput,
        start: &crate::ui::task_detail::RuntimeRecipeInput,
        approval_proof: crate::ui::layout::RuntimeTargetApprovalProof,
        readiness_port: &str,
        readiness_path: &str,
        expected_revision: u64,
    ) {
        let result = match &self.screen {
            Screen::Connected(project) => target_override::add(
                project,
                ticket,
                app_surface,
                project_relative_root,
                artifact_paths,
                build,
                test,
                start,
                approval_proof,
                readiness_port,
                readiness_path,
                expected_revision,
            ),
            Screen::Welcome => Err("Connect a project before adding a runtime target.".into()),
        };
        self.runtime_capability_result(result);
    }

    pub(super) fn discover_runtime_targets(&mut self, ticket: &str) {
        let result = match &self.screen {
            Screen::Connected(project) => (|| {
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
                    .ok_or_else(|| "Task has no isolated implementation checkout.".to_owned())?;
                if implementation
                    .task_uid
                    .as_deref()
                    .is_some_and(|uid| uid != metadata.uid)
                {
                    return Err("Task identity changed; reopen the task before discovery.".into());
                }
                let workspace_id = implementation
                    .workspace_uid
                    .clone()
                    .ok_or_else(|| "Task workspace identity is unavailable.".to_owned())?;
                let source_sha = implementation
                    .verified_head
                    .clone()
                    .ok_or_else(|| "Task source has no verified commit yet.".to_owned())?;
                let context = DiscoveryContext {
                    workspace_id,
                    repository_id: metadata.repository_id.clone(),
                    owning_task_id: metadata.uid.clone(),
                    source_sha,
                };
                let store = ProjectRuntimeStore::new(&project.state.planning_store);
                let revision = store
                    .load_manifest()
                    .map_err(|error| error.to_string())?
                    .map_or(0, |manifest| manifest.revision);
                store
                    .discover_and_save(&implementation.task_repository, &context, revision)
                    .map(|_| {
                        "Project target discovery completed. No project commands were run."
                            .to_owned()
                    })
                    .map_err(|error| error.to_string())
            })(),
            Screen::Welcome => Err("Connect a project before discovering runtime targets.".into()),
        };
        self.runtime_capability_result(result);
    }

    pub(super) fn select_runtime_target(
        &mut self,
        ticket: &str,
        target_id: &str,
        expected_revision: u64,
    ) {
        let result = match &self.screen {
            Screen::Connected(project) => (|| {
                let metadata = project
                    .task_documents
                    .iter()
                    .find(|document| document.path == ticket)
                    .and_then(|document| document.metadata.as_ref())
                    .ok_or_else(|| "Task identity is unavailable.".to_owned())?;
                let implementation = project
                    .implementation_states
                    .get(ticket)
                    .ok_or_else(|| "Task implementation is unavailable.".to_owned())?;
                let manifest = ProjectRuntimeStore::new(&project.state.planning_store)
                    .load_manifest()
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "Discover project targets before selecting one.".to_owned())?;
                validate_binding(
                    &manifest,
                    &metadata.uid,
                    &metadata.repository_id,
                    implementation,
                )?;
                ProjectRuntimeStore::new(&project.state.planning_store)
                    .select_target_and_save(&metadata.uid, target_id, expected_revision)
                    .map(|_| "Project target selected.".to_owned())
                    .map_err(|error| error.to_string())
            })(),
            Screen::Welcome => Err("Connect a project before selecting a runtime target.".into()),
        };
        self.runtime_capability_result(result);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn approve_runtime_recipe(
        &mut self,
        ticket: &str,
        target_id: &str,
        ui_operation: RuntimeRecipeOperation,
        approval_proof: crate::ui::layout::RuntimeTargetApprovalProof,
        executable_id: &str,
        argv_lines: &str,
        readiness_port: &str,
        readiness_path: &str,
        expected_revision: u64,
    ) {
        let result = match &self.screen {
            Screen::Connected(project) => {
                (|| {
                    let metadata = project
                        .task_documents
                        .iter()
                        .find(|document| document.path == ticket)
                        .and_then(|document| document.metadata.as_ref())
                        .ok_or_else(|| "Task identity is unavailable.".to_owned())?;
                    let implementation = project
                        .implementation_states
                        .get(ticket)
                        .ok_or_else(|| "Task implementation is unavailable.".to_owned())?;
                    let store = ProjectRuntimeStore::new(&project.state.planning_store);
                    let manifest = store
                        .load_manifest()
                        .map_err(|error| error.to_string())?
                        .ok_or_else(|| {
                            "Discover project targets before approving a recipe.".to_owned()
                        })?;
                    validate_binding(
                        &manifest,
                        &metadata.uid,
                        &metadata.repository_id,
                        implementation,
                    )?;
                    if !manifest.is_active_target(&metadata.uid, target_id) {
                        return Err(
                            "Select this task target explicitly before approving a recipe.".into(),
                        );
                    }
                    let target = manifest
                        .candidate_targets
                        .iter()
                        .find(|target| target.target_id == target_id)
                        .ok_or_else(|| "Selected target no longer exists.".to_owned())?;
                    let operation = match ui_operation {
                        RuntimeRecipeOperation::Build => RuntimeOperation::Build,
                        RuntimeRecipeOperation::Test => RuntimeOperation::Test,
                        RuntimeRecipeOperation::Start => RuntimeOperation::Start,
                    };
                    let argv: Vec<String> = argv_lines
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                        .map(str::to_owned)
                        .collect();
                    if argv.len() > 128 || argv.iter().any(|arg| arg.chars().any(char::is_control))
                    {
                        return Err("Recipe arguments contain control characters or exceed the argument limit.".into());
                    }
                    let approval_id = approval_proof.receipt_uid();
                    let recipe_id = format!("user-{}", approval_id.replace('-', ""));
                    let recipe = ExecutionRecipeV1 {
                        schema_version: 1,
                        recipe_id: recipe_id.clone(),
                        source: RecipeSource::UserApproved,
                        working_directory_relative: target.project_relative_root.clone(),
                        executable_id: executable_id.trim().to_owned(),
                        argv,
                        allowed_env_keys: Vec::new(),
                        allowed_output_paths: Vec::new(),
                        timeout_seconds: 1200,
                        expected_exit_codes: vec![0],
                        expected_artifacts: Vec::new(),
                        cache_scope: None,
                        prerequisite_tool_ids: vec![executable_id.trim().to_owned()],
                        network_requirement: NetworkRequirement::Denied,
                        evidence_refs: vec![format!("user-approval:{approval_id}")],
                    };
                    let readiness = if operation == RuntimeOperation::Start {
                        Some(match target.app_surface {
                            AppSurface::Web => ReadinessProbeV1::HttpOnApprovedPort {
                                port: readiness_port
                                    .trim()
                                    .parse()
                                    .map_err(|_| "Enter a valid approved web port.".to_owned())?,
                                path: readiness_path.trim().to_owned(),
                                expected_status: 200,
                            },
                            AppSurface::LinuxDesktop => ReadinessProbeV1::ProcessAlive {
                                executable_id: executable_id.trim().to_owned(),
                            },
                            AppSurface::HeadlessService => ReadinessProbeV1::LocalServiceHealth {
                                port: readiness_port.trim().parse().map_err(|_| {
                                    "Enter a valid approved service port.".to_owned()
                                })?,
                                path: readiness_path.trim().to_owned(),
                            },
                            AppSurface::UnsupportedTarget => {
                                return Err("Unsupported targets cannot be approved.".into());
                            }
                        })
                    } else {
                        None
                    };
                    store.approve_recipe_with_readiness_and_save(
                        target_id,
                        operation,
                        recipe,
                        readiness,
                        approval_proof,
                        expected_revision,
                    )
                    .map(|_| "Runtime recipe explicitly approved and saved. It has not been executed.".to_owned())
                    .map_err(|error| error.to_string())
                })()
            }
            Screen::Welcome => Err("Connect a project before approving runtime recipes.".into()),
        };
        self.runtime_capability_result(result);
    }

    fn runtime_capability_result(&mut self, result: Result<String, String>) {
        match result {
            Ok(message) => self.toasts.success(message),
            Err(error) => self
                .toasts
                .warning(format!("Runtime capability action failed: {error}")),
        }
    }
}

fn validate_binding(
    manifest: &crate::project_runtime::ProjectCapabilityManifestV1,
    task_uid: &str,
    repository_id: &str,
    implementation: &crate::core::implementation::Implementation,
) -> Result<(), String> {
    if implementation
        .task_uid
        .as_deref()
        .is_some_and(|uid| uid != task_uid)
        || implementation.workspace_uid.as_deref() != Some(manifest.workspace_id.as_str())
        || manifest.repository_id != repository_id
        || implementation.verified_head.as_deref() != Some(manifest.source_sha.as_str())
    {
        return Err("Runtime state is stale or belongs to a different task, workspace, repository, or source revision. Discover again.".into());
    }
    Ok(())
}
