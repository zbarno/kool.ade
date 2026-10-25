use crate::{
    execution::contract::ReadinessState,
    harness::discovery::{ModelCatalogStatus, ProbeResult, ProbeStatus},
};

use super::{Finding, dependencies};

pub(super) fn findings(probes: &[ProbeResult], selected_agent: Option<&str>) -> Vec<Finding> {
    let installed = probes
        .iter()
        .filter(|probe| probe.binary_path.is_some())
        .collect::<Vec<_>>();
    let verified = installed
        .iter()
        .copied()
        .filter(|probe| authenticated_context_agent(probe))
        .collect::<Vec<_>>();
    let mut findings = Vec::new();

    if installed.is_empty() {
        let crashed = probes
            .iter()
            .any(|probe| probe.status == ProbeStatus::Failed);
        findings.push(if crashed {
            Finding::unverified(
                "setup:cli:scan",
                "Check coding agent detection",
                "One of the coding agent checks did not finish reliably.",
                "Retry Coding Tools discovery before choosing an installer.",
            )
        } else {
            Finding::action(
                "setup:cli:install",
                "Choose a coding agent",
                "No supported coding agent is installed. Kool.ad/e can guide setup without one.",
                "Choose an agent in Coding Tools to get its official installation steps.",
            )
        });
    } else {
        // Only installed candidates produce configuration tasks.
        for probe in installed {
            let prefix = format!("setup:cli:{}", probe.provider_id);
            if probe.authentication == ReadinessState::Unavailable {
                findings.push(dependencies::requires(
                    Finding::action(
                        format!("{prefix}:auth"),
                        "Connect your coding agent account",
                        "The installed coding agent needs an authenticated account.",
                        "Sign in using the agent's official account flow and retry verification.",
                    ),
                    [dependencies::cli_install()],
                ));
            } else if probe.authentication == ReadinessState::Unknown {
                findings.push(dependencies::requires(
                    Finding::unverified(
                        format!("{prefix}:verify"),
                        "Verify coding agent access",
                        "This coding agent is installed, but its sign-in status could not be verified.",
                        "Review this agent in Coding Tools; unknown access is not treated as ready.",
                    ),
                    [dependencies::cli_install()],
                ));
            } else if probe.model_catalog_status != ModelCatalogStatus::Fresh
                || (probe.models.is_empty() && probe.default_model.is_none())
            {
                findings.push(dependencies::requires(
                    Finding::unverified(
                        format!("{prefix}:models"),
                        "Configure a usable AI model",
                        "This agent's current model availability is not verified.",
                        "Select a supported model and refresh this agent's model catalog.",
                    ),
                    [dependencies::cli_stage(&probe.provider_id, "auth")],
                ));
            } else if !authenticated_context_agent(probe) {
                findings.push(dependencies::requires(
                    Finding::unverified(
                        format!("{prefix}:capability"),
                        "Verify safe agent mode",
                        "The coding agent cannot yet be verified for context-only setup assistance.",
                        "Review its capability diagnostics; host tools remain disabled.",
                    ),
                    [dependencies::cli_stage(&probe.provider_id, "models")],
                ));
            }
        }
        if !verified.is_empty()
            && !selected_agent
                .is_some_and(|id| verified.iter().any(|probe| probe.provider_id == id))
        {
            findings.push(dependencies::requires(
                Finding::action(
                    "setup:cli:select",
                    "Choose your setup assistant",
                    "An agent is ready, but none has been explicitly selected.",
                    "Select an authenticated agent in Coding Tools.",
                ),
                verified
                    .iter()
                    .map(|probe| dependencies::cli_stage(&probe.provider_id, "capability")),
            ));
        }
    }
    findings
}

pub(super) fn checked_domains(probes: &[ProbeResult], selected_agent: Option<&str>) -> Vec<String> {
    let mut domains = Vec::new();
    if probes.iter().any(|probe| probe.binary_path.is_some()) {
        domains.push("cli:install".into());
    }
    if probes
        .iter()
        .all(|probe| probe.status != ProbeStatus::Failed)
    {
        domains.push("cli:scan".into());
    }
    for probe in probes.iter().filter(|probe| probe.binary_path.is_some()) {
        if probe.authentication == ReadinessState::Ready {
            domains.push(format!("cli:{}:auth", probe.provider_id));
        }
        if probe.model_catalog_status == ModelCatalogStatus::Fresh
            && (!probe.models.is_empty() || probe.default_model.is_some())
        {
            domains.push(format!("cli:{}:models", probe.provider_id));
        }
        if authenticated_context_agent(probe) {
            domains.push(format!("cli:{}:capability", probe.provider_id));
            domains.push(format!("cli:{}", probe.provider_id));
        }
    }
    if selected_agent.is_some_and(|id| {
        probes
            .iter()
            .any(|probe| probe.provider_id == id && authenticated_context_agent(probe))
    }) {
        domains.push("cli:select".into());
    }
    domains
}

fn authenticated_context_agent(probe: &ProbeResult) -> bool {
    probe.status == ProbeStatus::Available
        && probe.binary_path.is_some()
        && probe.version.is_some()
        && probe.authentication == ReadinessState::Ready
        && probe.execution_capability.safe_for_planning == ReadinessState::Ready
        && probe.model_catalog_status == ModelCatalogStatus::Fresh
        && (!probe.models.is_empty() || probe.default_model.is_some())
}
