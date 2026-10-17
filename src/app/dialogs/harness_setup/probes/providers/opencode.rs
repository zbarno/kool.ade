use super::HarnessProbe;
use crate::execution::contract::ReadinessState;

pub(super) fn discover(runtime_available: bool) -> HarnessProbe {
    let probe = crate::harness::OpenCodeHarness::probe_report();
    let ready = matches!(
        probe.readiness,
        crate::harness::opencode_harness::OpenCodeReadiness::Ready
            | crate::harness::opencode_harness::OpenCodeReadiness::CatalogUnavailable
    );
    let mut authentication = if probe.readiness
        == crate::harness::opencode_harness::OpenCodeReadiness::AuthenticationRequired
    {
        ReadinessState::Unavailable
    } else if let Some(binary) = probe.binary.as_deref() {
        super::opencode_authentication::probe(binary)
    } else {
        ReadinessState::Unknown
    };
    let models = if authentication == ReadinessState::Unavailable {
        super::opencode_local_models::models_available_without_authentication(&probe.models)
            .inspect(|_| authentication = ReadinessState::Ready)
            .unwrap_or(probe.models)
    } else {
        probe.models
    };
    let model_catalog_fresh = !models.is_empty();
    let auth_diagnostic = if authentication == ReadinessState::Unavailable
        && probe.readiness
            != crate::harness::opencode_harness::OpenCodeReadiness::AuthenticationRequired
    {
        Some("OpenCode authentication is not configured. Run `opencode auth login` and rediscover.")
    } else if authentication == ReadinessState::Unknown && probe.diagnostic.is_empty() {
        Some("OpenCode authentication could not be verified by `opencode auth list`.")
    } else {
        None
    };
    let diagnostic = [
        (!probe.diagnostic.is_empty()).then_some(probe.diagnostic),
        auth_diagnostic.map(str::to_owned),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    HarnessProbe {
        id: "opencode".into(),
        version: probe.version,
        executable: probe
            .binary
            .as_ref()
            .map(|path| path.to_string_lossy().into()),
        diagnostic: (!diagnostic.is_empty()).then(|| diagnostic.join("; ")),
        ready,
        models,
        default_model: None,
        model_catalog_fresh,
        configuration_required: authentication == ReadinessState::Unavailable
            || probe.readiness
                == crate::harness::opencode_harness::OpenCodeReadiness::ConfigurationRequired,
        authentication,
        implementation_available: ready && runtime_available,
        status: probe.status,
    }
}
