use crate::{
    execution::{
        contract::{ResourceBudgetV1, TargetPreference, WorkspaceRef},
        docker::images::RuntimeProfile,
        session::ExecutionSession,
    },
    harness::PlanningRequest,
    persistence::execution_environment_settings::TaskCapabilityRequirements,
};

use super::super::{can_fallback_for_preference, ensure_local_capabilities, local_session};

pub(super) fn auto_local_fallback(
    request: &PlanningRequest,
    workspace: WorkspaceRef,
    profile: RuntimeProfile,
    resources: ResourceBudgetV1,
    local_eligible: bool,
    requirements: TaskCapabilityRequirements,
    reason: &str,
) -> anyhow::Result<ExecutionSession> {
    ensure_local_capabilities(&requirements)?;
    anyhow::ensure!(
        can_fallback_for_preference(&TargetPreference::Auto, local_eligible),
        "{reason}; local Docker is unavailable"
    );
    local_session(request, workspace, profile, resources)
}
