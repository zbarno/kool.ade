//! Project-aware runtime resolution for local implementation sessions.
//!
//! Preference order for choosing the local runtime:
//! 1. **Approved project reality** — the logical tools required by the
//!    user-approved recipes of the task's active application targets (see
//!    `execution::docker::images::active_target_recipe_tools`). Selection
//!    then favors an existing locked image covering all tools, or falls
//!    through to verified on-demand composition when no single image does.
//! 2. **Legacy heuristic compatibility** — repository marker files
//!    (`Cargo.toml`, `package.json`, ...) when no usable manifest exists.
//!
//! The projection is best-effort by design: any failure to read or interpret
//! the planning store degrades to the heuristic instead of blocking task
//! start, while the execution boundary itself remains strictly validated
//! downstream (workspace binding, digest pins, non-root, offline network).

use crate::{
    execution::docker::images::{RuntimePlanBinding, RuntimeProfile, active_target_recipe_tools},
    harness::PlanningRequest,
};

/// Outcome of local runtime resolution for an implementation session.
pub(super) enum LocalRuntimePlan {
    /// Tools required by the approved recipe set of the task's active
    /// targets, bound to the project facts that produced them.
    Project {
        tools: Vec<String>,
        binding: RuntimePlanBinding,
    },
    /// Compatibility fallback using the historical marker-file heuristic.
    Heuristic(RuntimeProfile),
}

pub(super) fn resolve_local_runtime(
    request: &PlanningRequest,
    fallback_profile: RuntimeProfile,
) -> LocalRuntimePlan {
    match project_runtime_plan(request) {
        Some(plan) => plan,
        None => LocalRuntimePlan::Heuristic(fallback_profile),
    }
}

fn project_runtime_plan(request: &PlanningRequest) -> Option<LocalRuntimePlan> {
    let workspace = request.execution_workspace.as_ref()?;
    let task_id = request.task_id.as_deref()?;
    let project_uuid = uuid::Uuid::parse_str(&workspace.project_id).ok()?;
    let planning_store = crate::artifacts::planning_store::PlanningStore::new(
        project_uuid,
        crate::persistence::state_root(),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let manifest = crate::project_runtime::ProjectRuntimeStore::new(&planning_store)
        .load_manifest()
        .ok()
        .flatten()?;
    let tools = active_target_recipe_tools(&manifest, task_id)?;
    let binding = RuntimePlanBinding::new(
        workspace.project_id.clone(),
        manifest.repository_id,
        manifest.source_sha,
        manifest.selected_target_id.clone(),
        tools.recipe_ids.clone(),
    );
    Some(LocalRuntimePlan::Project {
        tools: tools.tools,
        binding,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_request() -> PlanningRequest {
        let (_tx, _rx) = std::sync::mpsc::channel();
        PlanningRequest {
            investigation_authorization: None,
            prompt_manifest: None,
            mode: crate::harness::ExecutionMode::Implementation,
            task_id: Some("task-x".to_owned()),
            reasoning_level: "high".into(),
            telemetry_phase: None,
            repo_root: std::env::temp_dir(),
            runtime_config_source: None,
            prompt_body: "fixture task".into(),
            system_instructions: "fixture policy".into(),
            timeout: std::time::Duration::from_secs(5),
            progress_tx: _tx,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            execution_workspace: None,
            destination_sha: None,
            authorization_verifier: None,
            review_identity: None,
            computer_use: None,
        }
    }

    #[test]
    fn resolution_degrades_to_heuristic_without_a_task_identity() {
        let mut request = minimal_request();
        request.execution_workspace = Some(crate::execution::contract::WorkspaceRef {
            project_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            task_id: "task-x".to_owned(),
            source_sha: "a".repeat(40),
        });
        request.task_id = None;
        assert!(matches!(
            resolve_local_runtime(&request, RuntimeProfile::General),
            LocalRuntimePlan::Heuristic(RuntimeProfile::General)
        ));
    }
}
