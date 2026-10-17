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
    let (failure_stage, diagnostic_code) = if report.executable.is_none() {
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
    } else if !report.ready {
        (
            Some(ProbeKind::Capability),
            Some(ProbeDiagnosticCode::ProviderProbeFailed),
        )
    } else {
        (None, None)
    };
    let status = if report.ready {
        ProbeStatus::Available
    } else if report.authentication == ReadinessState::Unavailable {
        ProbeStatus::NeedsAuthentication
    } else if report.executable.is_some() {
        ProbeStatus::Installed
    } else {
        ProbeStatus::Missing
    };
    let ready = if report.ready {
        ReadinessState::Ready
    } else {
        ReadinessState::Unavailable
    };
    let implementation = if report.implementation_available {
        ReadinessState::Ready
    } else if report.ready {
        ReadinessState::Unknown
    } else {
        ReadinessState::Unavailable
    };
    let message = report
        .diagnostic
        .as_deref()
        .map(crate::error::redact_secrets)
        .filter(|message| !message.trim().is_empty());
    let model_catalog_status = model_catalog_status(report);
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
            safe_for_planning: ready,
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
