use crate::harness::{HarnessRouteProvenance, PlanningRequest};

pub(super) struct InvocationOutcome<'a> {
    pub status: &'a str,
    pub failure_category: Option<&'a str>,
    pub failure_message: Option<&'a str>,
}

pub(super) fn failure_category(error: &crate::error::AppError) -> String {
    match error {
        crate::error::AppError::HarnessExecutionFailed { failure } => {
            format!("{:?}", failure.category).to_ascii_lowercase()
        }
        crate::error::AppError::HarnessTimedOut { .. } => "timed_out".into(),
        crate::error::AppError::HarnessFailed { reason, .. }
            if reason.to_ascii_lowercase().contains("cancel") =>
        {
            "cancelled".into()
        }
        crate::error::AppError::HarnessFailed { .. } => "provider_failure".into(),
        crate::error::AppError::ProviderCapabilityMismatch { .. } => {
            "unsupported_provider_capability".into()
        }
        _ => "application_failure".into(),
    }
}

pub(super) fn persist_invocation(
    project_root: &std::path::Path,
    harness_label: &str,
    route: Option<&HarnessRouteProvenance>,
    request: &PlanningRequest,
    execution_id: &str,
    outcome: InvocationOutcome<'_>,
) {
    let project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let (harness, version) = harness_label
        .split_once(' ')
        .map_or((harness_label, None), |(name, version)| {
            (name, Some(version.to_owned()))
        });
    let mut record = crate::persistence::telemetry::InvocationRecord::new(
        crate::persistence::project_slug(&project_root),
        harness,
    );
    record.session = Some(execution_id.to_owned());
    record.execution_id = Some(execution_id.to_owned());
    record.task = request.task_id.clone();
    record.phase = Some("manager".into());
    record.harness_version = version;
    record.thinking_level = Some(request.reasoning_level.clone());
    record.execution_mode = Some(format!("{:?}", request.mode));
    record.execution_target = request
        .execution_workspace
        .as_ref()
        .map(|workspace| workspace.task_id.clone());
    record.failure_category = outcome.failure_category.map(str::to_owned);
    record.failure_message = outcome.failure_message.map(str::to_owned);
    if let Some(route) = route {
        record.routed_harness = Some(route.provider_id.clone());
        record.route_role = Some(format!("{:?}", route.role));
        record.work_category = route.work_category.clone();
        record.model = route.selected_model.clone();
        record.requested_model = route.requested_model.clone();
        record.capability_probe_id = route.capability_probe_id.clone();
        record.capability_generation = route.capability_generation;
    }
    record.outcome = outcome.status.to_owned();
    let _ = crate::persistence::telemetry::append(&record.project, &record);
}
