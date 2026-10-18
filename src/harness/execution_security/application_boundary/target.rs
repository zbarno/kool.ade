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
    ensure_settings_available(diagnostic.as_deref())?;
    let preference = selected_preference(&settings, request.task_id.as_deref());
    let requirements = task_capability_requirements(&settings, request.task_id.as_deref());
    if matches!(preference, TargetPreference::Local) {
        ensure_local_capabilities(&requirements)?;
    }
    match preference {
        TargetPreference::Local => {
            ensure_local_capabilities(&requirements)?;
            local_session(request, workspace, profile, resources)
        }
        TargetPreference::Worker(worker_id) => remote::remote_session(
            request,
            workspace,
            profile,
            resources,
            &worker_id,
            requirements,
        ),
        TargetPreference::Auto => {
            automatic::auto_session(request, workspace, profile, resources, requirements)
        }
    }
}

fn ensure_settings_available(diagnostic: Option<&str>) -> anyhow::Result<()> {
    if let Some(diagnostic) = diagnostic {
        anyhow::bail!("Execution target settings are invalid: {diagnostic}");
    }
    Ok(())
}

fn ensure_local_capabilities(
    requirements: &crate::persistence::execution_environment_settings::TaskCapabilityRequirements,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !requirements.requires_gpu && !requirements.requires_display,
        "Local Docker does not provide the GPU or display capability required by this task"
    );
    Ok(())
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

fn task_capability_requirements(
    settings: &ExecutionEnvironmentSettings,
    task_id: Option<&str>,
) -> crate::persistence::execution_environment_settings::TaskCapabilityRequirements {
    task_id
        .and_then(|task_id| settings.task_capabilities.get(task_id))
        .copied()
        .unwrap_or_default()
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
    ExecutionSession::connect_local_with_destination_sha_and_authorization(
        workspace,
        &request.repo_root,
        ExecutionModeV1::Implementation,
        profile,
        resources,
        request.destination_sha.clone(),
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
    fn local_target_rejects_unavailable_task_capabilities() {
        let requirements =
            crate::persistence::execution_environment_settings::TaskCapabilityRequirements {
                requires_gpu: false,
                requires_display: true,
            };
        assert!(ensure_local_capabilities(&requirements).is_err());
    }

    #[test]
    fn task_capability_requirements_are_selected_by_stable_task_id() {
        let requirements =
            crate::persistence::execution_environment_settings::TaskCapabilityRequirements {
                requires_gpu: true,
                requires_display: false,
            };
        let settings = ExecutionEnvironmentSettings {
            task_capabilities: [("task-123".into(), requirements)].into(),
            ..ExecutionEnvironmentSettings::default()
        };
        assert_eq!(
            task_capability_requirements(&settings, Some("task-123")),
            requirements
        );
        assert_eq!(
            task_capability_requirements(&settings, Some("another-task")),
            Default::default()
        );
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
            task_capabilities: Default::default(),
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

    #[test]
    fn invalid_target_settings_block_execution_instead_of_falling_back() {
        let error = ensure_settings_available(Some("invalid settings"))
            .expect_err("unknown task capability requirements must block execution");
        assert!(
            error
                .to_string()
                .contains("Execution target settings are invalid")
        );
        assert!(ensure_settings_available(None).is_ok());
    }
}
