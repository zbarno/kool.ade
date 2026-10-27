use super::HarnessProbe;
use crate::execution::contract::ReadinessState;

pub(super) fn discover(runtime_available: bool) -> HarnessProbe {
    let probe = crate::harness::OpenCodeHarness::probe_report();
    let ready = matches!(
        probe.readiness,
        crate::harness::opencode_harness::OpenCodeReadiness::Ready
            | crate::harness::opencode_harness::OpenCodeReadiness::CatalogUnavailable
    );
    let local_evidence = super::opencode_local_models::probe_local_models(&probe.models);
    // For known local-only providers, authentication Ready means no credentials are required;
    // only a fresh catalog response that lists a configured model can make it routable. Remote
    // Anthropic access uses a separate non-generative key and model-list check.
    let (authentication, models, model_catalog_fresh, local_diagnostic) = match local_evidence {
        super::opencode_local_models::LocalModelEvidence::NotConfigured => {
            if let Some(binary) = probe.binary.as_deref() {
                let evidence =
                    super::opencode_authentication::verify_anthropic(binary, &probe.models);
                (
                    evidence.authentication,
                    evidence.models.unwrap_or(probe.models),
                    evidence.model_catalog_fresh,
                    evidence.diagnostic,
                )
            } else {
                (ReadinessState::Unknown, probe.models, false, None)
            }
        }
        super::opencode_local_models::LocalModelEvidence::EndpointUnavailable => {
            // For these documented local-only providers, authentication is not required. The
            // stale catalog still blocks routing until the endpoint returns fresh model evidence.
            (
                ReadinessState::Ready,
                probe.models,
                false,
                Some(
                    "The configured local OpenCode endpoint did not return a usable model catalog. Start it and retry discovery.",
                ),
            )
        }
        super::opencode_local_models::LocalModelEvidence::AuthenticationRequired => (
            ReadinessState::Unavailable,
            probe.models,
            false,
            Some(
                "The configured local OpenCode endpoint rejected an unauthenticated model-catalog request.",
            ),
        ),
        super::opencode_local_models::LocalModelEvidence::FreshCatalog(models) => {
            let diagnostic = models.is_empty().then_some(
                "The live local OpenCode catalog does not list the configured model. Choose an available model and retry discovery.",
            );
            (ReadinessState::Ready, models, true, diagnostic)
        }
    };
    let auth_diagnostic = if local_diagnostic.is_none()
        && authentication == ReadinessState::Unavailable
        && probe.readiness
            != crate::harness::opencode_harness::OpenCodeReadiness::AuthenticationRequired
    {
        Some("OpenCode provider access is unverified. Sign in with the provider, then rediscover.")
    } else if authentication == ReadinessState::Unknown && probe.diagnostic.is_empty() {
        Some("OpenCode authentication could not be verified by `opencode auth list`.")
    } else {
        None
    };
    let diagnostic = [
        (!probe.diagnostic.is_empty()).then_some(probe.diagnostic),
        local_diagnostic.map(str::to_owned),
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
