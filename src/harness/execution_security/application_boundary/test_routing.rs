use super::target;
use crate::{
    execution::{
        contract::{ResourceBudgetV1, TargetPreference, WorkspaceRef},
        docker::images::RuntimeProfile,
        session::ExecutionSession,
    },
    harness::PlanningRequest,
    persistence::execution_environment_settings::TaskCapabilityRequirements,
};

pub(crate) fn auto_remote_session_for_retry_test(
    request: &PlanningRequest,
    workspace: WorkspaceRef,
    profile: RuntimeProfile,
    resources: ResourceBudgetV1,
    requirements: TaskCapabilityRequirements,
    allowed_targets: Vec<TargetPreference>,
    excluded_workers: Vec<String>,
) -> anyhow::Result<ExecutionSession> {
    target::auto_session_for_retry_test(
        request,
        workspace,
        profile,
        resources,
        requirements,
        allowed_targets,
        excluded_workers,
    )
}
