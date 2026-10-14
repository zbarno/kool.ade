mod automatic;
mod remote;

use crate::{
    execution::{
        contract::{ExecutionModeV1, ResourceBudgetV1, TargetPreference},
        docker::images::RuntimeProfile,
        session::ExecutionSession,
    },
    harness::PlanningRequest,
};

use crate::persistence::execution_environment_settings::ExecutionEnvironmentSettings;

pub(super) fn connect(
    request: &PlanningRequest,
    workspace: crate::execution::contract::WorkspaceRef,
    profile: RuntimeProfile,
    resources: ResourceBudgetV1,
) -> anyhow::Result<ExecutionSession> {
    let (settings, diagnostic) = crate::persistence::execution_environment_settings::load();
    let preference = selected_preference(&settings, request.task_id.as_deref());
    if let Some(diagnostic) = diagnostic {
        if matches!(preference, TargetPreference::Worker(_)) {
            anyhow::bail!("Remote target settings are invalid: {diagnostic}");
        }
        if matches!(preference, TargetPreference::Auto) {
            return local_session(request, workspace, profile, resources).map_err(|_| {
                anyhow::anyhow!("Execution target settings are invalid: {diagnostic}")
            });
        }
    }
    match preference {
        TargetPreference::Local => local_session(request, workspace, profile, resources),
        TargetPreference::Worker(worker_id) => {
            remote::remote_session(request, workspace, profile, resources, &worker_id)
        }
        TargetPreference::Auto => automatic::auto_session(request, workspace, profile, resources),
    }
}

fn selected_preference(
    settings: &ExecutionEnvironmentSettings,
    task_id: Option<&str>,
) -> TargetPreference {
    task_id
        .and_then(|task_id| settings.task_targets.get(task_id))
        .cloned()
        .unwrap_or_else(|| settings.default_target.clone())
}

fn can_fallback_for_preference(preference: &TargetPreference, local_eligible: bool) -> bool {
    matches!(preference, TargetPreference::Auto) && local_eligible
}

fn local_session(
    request: &PlanningRequest,
    workspace: crate::execution::contract::WorkspaceRef,
    profile: RuntimeProfile,
    resources: ResourceBudgetV1,
) -> anyhow::Result<ExecutionSession> {
    ExecutionSession::connect_local_with_authorization(
        workspace,
        &request.repo_root,
        ExecutionModeV1::Implementation,
        profile,
        resources,
        request.authorization_verifier.clone(),
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "Docker implementation session unavailable: {}",
            error.message
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_worker_never_has_local_fallback() {
        assert!(!can_fallback_for_preference(
            &TargetPreference::Worker("worker-x".into()),
            true
        ));
    }

    #[test]
    fn auto_fallback_requires_local_eligibility() {
        assert!(can_fallback_for_preference(&TargetPreference::Auto, true));
        assert!(!can_fallback_for_preference(&TargetPreference::Auto, false));
    }

    #[test]
    fn task_target_overrides_default_only_for_that_task() {
        let settings = ExecutionEnvironmentSettings {
            schema_version: 1,
            default_target: TargetPreference::Local,
            task_targets: [(
                "task-123".to_owned(),
                TargetPreference::Worker("worker-healthy".into()),
            )]
            .into(),
        };

        assert_eq!(
            selected_preference(&settings, Some("task-123")),
            TargetPreference::Worker("worker-healthy".into())
        );
        assert_eq!(
            selected_preference(&settings, Some("task-other")),
            TargetPreference::Local
        );
        assert_eq!(
            selected_preference(&settings, None),
            TargetPreference::Local
        );
    }
}
