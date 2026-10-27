use super::HarnessProbe;
use crate::execution::contract::ReadinessState;
use crate::harness::discovery::{
    ModelCatalogStatus, ProbeDiagnosticCode, ProbeKind, ProbeResult, ProbeStatus,
};

#[cfg(test)]
mod tests;

pub(super) fn from_harness_probe(
    report: &HarnessProbe,
    generation: u64,
    started_at_unix_ms: u64,
    duration_ms: u64,
) -> ProbeResult {
    let catalog_error = has_catalog_error(report);
    let invalid_path = invalid_configured_path(report);
    let unsupported_version = unsupported_version(report);
    let execution_block =
        crate::harness::execution_security::task_execution_block_reason(&report.id);
    let (failure_stage, diagnostic_code) = if invalid_path {
        (
            Some(ProbeKind::Executable),
            Some(ProbeDiagnosticCode::ExecutableInvalid),
        )
    } else if unsupported_version {
        (
            Some(ProbeKind::Version),
            Some(ProbeDiagnosticCode::UnsupportedVersion),
        )
    } else if report.executable.is_none() {
        (
            Some(ProbeKind::Executable),
            Some(ProbeDiagnosticCode::ExecutableMissing),
        )
    } else if report.version.is_none() {
        (
            Some(ProbeKind::Version),
            Some(ProbeDiagnosticCode::VersionUnavailable),
        )
    } else if report.authentication == ReadinessState::Unavailable {
        (
            Some(ProbeKind::Authentication),
            Some(ProbeDiagnosticCode::AuthenticationRequired),
        )
    } else if report.configuration_required {
        (
            Some(ProbeKind::Models),
            Some(ProbeDiagnosticCode::ConfigurationRequired),
        )
    } else if catalog_error {
        (
            Some(ProbeKind::Models),
            Some(ProbeDiagnosticCode::ModelCatalogUnavailable),
        )
    } else if execution_block.is_some() {
        (Some(ProbeKind::Capability), None)
    } else if !report.ready {
        (
            Some(ProbeKind::Capability),
            Some(ProbeDiagnosticCode::ProviderProbeFailed),
        )
    } else {
        (None, None)
    };
    let ready = if unsupported_version || invalid_path {
        ReadinessState::Unavailable
    } else if report.ready && report.authentication == ReadinessState::Ready {
        ReadinessState::Ready
    } else if !report.ready || report.authentication == ReadinessState::Unavailable {
        ReadinessState::Unavailable
    } else {
        ReadinessState::Unknown
    };
    let model_catalog_status = model_catalog_status(report);
    let has_model = !report.models.is_empty() || report.default_model.is_some();
    let context_readiness = if execution_block.is_some() {
        ReadinessState::Unavailable
    } else if ready == ReadinessState::Ready
        && model_catalog_status == ModelCatalogStatus::Fresh
        && has_model
    {
        ReadinessState::Ready
    } else if ready == ReadinessState::Ready {
        ReadinessState::Unknown
    } else {
        ready
    };
    let status = if invalid_path {
        ProbeStatus::Failed
    } else if unsupported_version {
        ProbeStatus::Installed
    } else if context_readiness == ReadinessState::Ready {
        ProbeStatus::Available
    } else if report.authentication == ReadinessState::Unavailable {
        ProbeStatus::NeedsAuthentication
    } else if report.executable.is_some() {
        ProbeStatus::Installed
    } else {
        ProbeStatus::Missing
    };
    let implementation = if execution_block.is_some() {
        ReadinessState::Unavailable
    } else if report.implementation_available && context_readiness == ReadinessState::Ready {
        ReadinessState::Ready
    } else if context_readiness != ReadinessState::Unavailable {
        ReadinessState::Unknown
    } else {
        ReadinessState::Unavailable
    };
    let message = report
        .diagnostic
        .as_deref()
        .map(crate::error::redact_secrets)
        .filter(|message| !message.trim().is_empty())
        .or_else(|| execution_block.map(str::to_owned));
    ProbeResult {
        schema_version: 1,
        provider_id: report.id.clone(),
        run_generation: generation,
        started_at_unix_ms,
        duration_ms,
        binary_path: report.executable.clone(),
        version: report.version.clone(),
        authentication: report.authentication,
        model_catalog_status,
        models: report.models.clone(),
        default_model: report.default_model.clone(),
        execution_capability: crate::harness::discovery::ExecutionCapability {
            safe_for_planning: context_readiness,
            safe_for_implementation: implementation,
            runtime_ready: if report.implementation_available {
                ReadinessState::Ready
            } else {
                ReadinessState::Unknown
            },
        },
        status,
        display_status: report.status.clone(),
        failure_stage,
        diagnostic_code,
        diagnostic_message: message,
    }
    .bounded()
}

pub(super) fn internal_failure_result(
    provider_id: &str,
    generation: u64,
    started_at_unix_ms: u64,
    duration_ms: u64,
) -> ProbeResult {
    ProbeResult {
        provider_id: provider_id.into(),
        run_generation: generation,
        started_at_unix_ms,
        duration_ms,
        execution_capability: crate::harness::discovery::ExecutionCapability {
            safe_for_planning: ReadinessState::Unavailable,
            safe_for_implementation: ReadinessState::Unavailable,
            runtime_ready: ReadinessState::Unknown,
        },
        status: crate::harness::discovery::ProbeStatus::Failed,
        display_status: format!("{provider_id} (probe failed)"),
        failure_stage: Some(ProbeKind::Capability),
        diagnostic_code: Some(ProbeDiagnosticCode::InternalProbeFailure),
        diagnostic_message: Some(
            "Provider discovery failed unexpectedly. Retry this provider.".into(),
        ),
        ..ProbeResult::default()
    }
    .bounded()
}

pub(super) fn model_catalog_status(report: &HarnessProbe) -> ModelCatalogStatus {
    if report.model_catalog_fresh {
        ModelCatalogStatus::Fresh
    } else if !report.models.is_empty() {
        ModelCatalogStatus::Stale
    } else if has_catalog_error(report) {
        ModelCatalogStatus::Unavailable
    } else {
        ModelCatalogStatus::NotChecked
    }
}

fn has_catalog_error(report: &HarnessProbe) -> bool {
    report.diagnostic.as_deref().is_some_and(|message| {
        let message = message.to_ascii_lowercase();
        message.contains("catalog") || message.contains("models")
    })
}

fn invalid_configured_path(report: &HarnessProbe) -> bool {
    if report
        .status
        .to_ascii_lowercase()
        .contains("invalid executable path")
    {
        return true;
    }
    report.executable.is_none()
        && report.diagnostic.as_deref().is_some_and(|message| {
            let message = message.to_ascii_lowercase();
            message.contains("configured")
                && message.contains("path")
                && ["does not exist", "must be absolute", "not an executable"]
                    .iter()
                    .any(|needle| message.contains(needle))
        })
}

fn unsupported_version(report: &HarnessProbe) -> bool {
    let status = report.status.to_ascii_lowercase();
    let diagnostic = report
        .diagnostic
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    status.contains("update required")
        || diagnostic.contains("unsupported for clean execution")
        || diagnostic.contains("profile requires version")
        || diagnostic.contains("missing required capabilities")
        || diagnostic.contains("version evidence is not parseable")
}

pub(super) fn into_harness_probe(result: &ProbeResult) -> HarnessProbe {
    let diagnostic = result.diagnostic_message.as_ref().map(|message| {
        match (result.failure_stage, result.diagnostic_code) {
            (Some(stage), Some(code)) => {
                format!("{} · {}: {message}", stage.label(), code.label())
            }
            (Some(stage), None) => format!("{}: {message}", stage.label()),
            _ => message.clone(),
        }
    });
    HarnessProbe {
        id: result.provider_id.clone(),
        status: result.display_status.clone(),
        version: result.version.clone(),
        executable: result.binary_path.clone(),
        diagnostic,
        ready: result.execution_capability.safe_for_planning == ReadinessState::Ready,
        models: result.models.clone(),
        default_model: result.default_model.clone(),
        model_catalog_fresh: result.model_catalog_status == ModelCatalogStatus::Fresh,
        configuration_required: matches!(
            result.diagnostic_code,
            Some(
                ProbeDiagnosticCode::AuthenticationRequired
                    | ProbeDiagnosticCode::ConfigurationRequired
            )
        ),
        authentication: result.authentication,
        implementation_available: result.execution_capability.safe_for_implementation
            == ReadinessState::Ready,
    }
}
