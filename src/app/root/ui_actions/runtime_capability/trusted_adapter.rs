use super::super::{KooladeApp, Screen};
use crate::project_runtime::{
    ExecutionRecipeV1, NetworkRequirement, ProjectRuntimeStore, RecipeSource, RuntimeOperation,
    trusted_adapter_template,
};

impl KooladeApp {
    pub(in crate::app::root::ui_actions) fn apply_runtime_adapter(
        &mut self,
        ticket: &str,
        target_id: &str,
        ui_operation: crate::ui::task_detail::RuntimeRecipeOperation,
        expected_revision: u64,
    ) {
        let result = match &self.screen {
            Screen::Connected(project) => (|| {
                use crate::project_runtime::AppSurface;
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
                let store = ProjectRuntimeStore::new(&project.state.planning_store);
                let manifest = store
                    .load_manifest()
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| {
                        "Discover project targets before applying an adapter.".to_owned()
                    })?;
                super::validate_binding(
                    &manifest,
                    &metadata.uid,
                    &metadata.repository_id,
                    implementation,
                )?;
                if !manifest.is_active_target(&metadata.uid, target_id) {
                    return Err(
                        "Select this task target explicitly before applying an adapter.".into(),
                    );
                }
                let target = manifest
                    .candidate_targets
                    .iter()
                    .find(|target| target.target_id == target_id)
                    .ok_or_else(|| "Selected target no longer exists.".to_owned())?;
                if target.app_surface == AppSurface::UnsupportedTarget {
                    return Err("Unsupported targets cannot use runtime adapters.".into());
                }
                let operation = match ui_operation {
                    crate::ui::task_detail::RuntimeRecipeOperation::Test => RuntimeOperation::Test,
                    _ => {
                        return Err(
                            "Trusted adapters are currently available for test recipes only."
                                .into(),
                        );
                    }
                };

                // Reinspect the selected checkout so a manifest edit cannot
                // claim a language unsupported by metadata at this target root.
                let evidence = crate::project_runtime::detect_candidate_targets(
                    &implementation.task_repository,
                )
                .map_err(|error| format!("Target metadata could not be verified: {error}"))?;
                let language = evidence
                    .iter()
                    .find(|candidate| {
                        crate::project_runtime::target_id_for_evidence_path(
                            &candidate.evidence_path,
                        ) == target_id
                    })
                    .map(|candidate| candidate.language.as_str());
                let template = trusted_adapter_template(language, operation).ok_or_else(|| {
                    "No trusted test adapter matches metadata at this target root.".to_owned()
                })?;
                if !target.test_recipe_refs.is_empty()
                    || target.build_recipe_ref.is_some()
                    || target.start_recipe_ref.is_some()
                {
                    return Err("Trusted adapters can only be applied to a target without existing recipes.".into());
                }
                let recipe = ExecutionRecipeV1 {
                    schema_version: 1,
                    recipe_id: format!("adapter-{}", uuid::Uuid::new_v4().simple()),
                    source: RecipeSource::TrustedAdapter,
                    working_directory_relative: target.project_relative_root.clone(),
                    executable_id: template.executable_id.into(),
                    argv: template.argv.iter().map(|arg| (*arg).into()).collect(),
                    allowed_env_keys: Vec::new(),
                    allowed_output_paths: Vec::new(),
                    timeout_seconds: 1200,
                    expected_exit_codes: vec![0],
                    expected_artifacts: Vec::new(),
                    cache_scope: None,
                    prerequisite_tool_ids: template
                        .prerequisite_tool_ids
                        .iter()
                        .map(|tool| (*tool).into())
                        .collect(),
                    network_requirement: NetworkRequirement::Denied,
                    evidence_refs: vec![format!("trusted-adapter:{}", template.id)],
                };
                store
                    .add_trusted_adapter_recipe_and_save(
                        target_id,
                        operation,
                        recipe,
                        expected_revision,
                    )
                    .map(|_| {
                        "Trusted test adapter explicitly applied. The recipe has not been executed."
                            .to_owned()
                    })
                    .map_err(|error| error.to_string())
            })(),
            Screen::Welcome => Err("Connect a project before applying a runtime adapter.".into()),
        };
        match result {
            Ok(message) => self.toasts.success(message),
            Err(error) => self
                .toasts
                .warning(format!("Runtime adapter failed: {error}")),
        }
    }
}
