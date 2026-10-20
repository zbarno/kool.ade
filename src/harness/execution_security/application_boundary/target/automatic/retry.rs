use super::*;
use crate::execution::session::RemoteRetryFactory;
use std::sync::Arc;

#[allow(clippy::too_many_arguments)]
pub(super) fn install_factory(
    session: &ExecutionSession,
    request: &PlanningRequest,
    workspace: &crate::execution::contract::WorkspaceRef,
    profile: RuntimeProfile,
    resources: &ResourceBudgetV1,
    requirements: crate::persistence::execution_environment_settings::TaskCapabilityRequirements,
    allowed_targets: &[TargetPreference],
    excluded_workers: &[String],
    worker_id: String,
) -> anyhow::Result<()> {
    let retry_request = request.clone();
    let retry_workspace = workspace.clone();
    let retry_resources = resources.clone();
    let retry_allowed_targets = allowed_targets.to_vec();
    let mut retry_excluded_workers = excluded_workers.to_vec();
    retry_excluded_workers.push(worker_id);
    let factory: Arc<RemoteRetryFactory> = Arc::new(move || {
        let next = auto_session_excluding(
            &retry_request,
            retry_workspace.clone(),
            profile,
            retry_resources.clone(),
            requirements,
            retry_allowed_targets.clone(),
            retry_excluded_workers.clone(),
        )
        .map_err(|error| {
            crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
                format!("No alternate trusted remote worker is available: {error}"),
                true,
            )
        })?;
        Ok(Arc::new(next))
    });
    session
        .set_remote_retry_factory(factory)
        .map_err(|error| anyhow::anyhow!("{error}"))
}
