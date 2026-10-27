use crate::execution::contract::ReadinessState;
use crate::harness::discovery::{
    ModelCatalogStatus, ProbeDiagnosticCode, ProbeKind, ProbeResult, ProbeStatus,
};

use super::super::{Finding, ProviderIdV1};

pub(super) fn authenticated_context_agent(probe: &ProbeResult, generation: u64) -> bool {
    probe.status == ProbeStatus::Available
        && probe.run_generation == generation
        && !matches!(
            probe.diagnostic_code,
            Some(ProbeDiagnosticCode::ExecutableInvalid | ProbeDiagnosticCode::UnsupportedVersion)
        )
        && probe.failure_stage != Some(ProbeKind::Version)
        && probe.binary_path.is_some()
        && probe.version.is_some()
        && probe.authentication == ReadinessState::Ready
        && crate::harness::execution_security::task_execution_block_reason(&probe.provider_id)
            .is_none()
        && probe.execution_capability.safe_for_planning == ReadinessState::Ready
        && probe.model_catalog_status == ModelCatalogStatus::Fresh
        && (!probe.models.is_empty() || probe.default_model.is_some())
}

pub(super) fn choose_provider() -> Finding {
    Finding::action(
        "setup:cli:install",
        "Choose a coding agent to set up",
        "Kool.ad/e found no explicitly chosen setup agent. It will not silently select a provider or sign in for you.",
        "Open Coding Tools, prioritize one provider for setup tasks, then follow its official installation and sign-in guidance.",
    )
}

pub(super) fn selected_provider_findings(
    provider_id: &str,
    probes: &[ProbeResult],
    generation: u64,
) -> Vec<Finding> {
    let Some(provider) = ProviderIdV1::from_probe_id(provider_id) else {
        return vec![Finding::unverified(
            "setup:cli:verify",
            "Verify the selected coding agent",
            "The selected provider is not recognized by this version of Kool.ad/e.",
            "Choose a supported provider in Coding Tools, then retry machine setup discovery.",
        )];
    };
    let name = provider.display_name();
    let prefix = format!("setup:cli:{provider_id}");
    let Some(probe) = probes.iter().find(|probe| probe.provider_id == provider_id) else {
        return vec![Finding::unverified(
            format!("{prefix}:verify"),
            &format!("Retry {name} discovery"),
            "The selected provider did not return a result in the latest machine scan.",
            "Retry Coding Tools discovery before relying on this provider.",
        )];
    };

    if probe.run_generation != generation {
        return vec![Finding::unverified(
            format!("{prefix}:verify"),
            "Refresh coding agent evidence",
            "This provider result belongs to an earlier machine scan.",
            "Retry Coding Tools discovery before relying on this provider.",
        )];
    }

    if probe.diagnostic_code == Some(ProbeDiagnosticCode::ExecutableInvalid) {
        return vec![Finding::action(
            format!("{prefix}:path"),
            &format!("Choose a valid {name} executable"),
            "The selected executable path does not identify a usable provider CLI.",
            "Review the configured path in Coding Tools or return to automatic discovery.",
        )];
    }

    if probe.diagnostic_code == Some(ProbeDiagnosticCode::UnsupportedVersion)
        || probe.failure_stage == Some(crate::harness::discovery::ProbeKind::Version)
    {
        return vec![Finding::action(
            format!("{prefix}:version"),
            &format!("Update {name}"),
            "This provider version is missing or does not support Kool.ad/e's verified invocation mode.",
            "Use the provider's official installation guide to install a supported version, then rediscover it.",
        )];
    }

    if probe.binary_path.is_none() {
        return vec![if probe.status == ProbeStatus::Missing {
            Finding::action(
                format!("{prefix}:install"),
                &format!("Install {name}"),
                "The provider is not installed at the selected path.",
                "Follow the official provider installation guide for this operating system, then rediscover it.",
            )
        } else {
            Finding::unverified(
                format!("{prefix}:verify"),
                &format!("Verify {name}"),
                "Kool.ad/e could not determine whether this provider is installed.",
                "Review the redacted discovery status in Coding Tools and retry the provider check.",
            )
        }];
    }

    let mut findings = Vec::new();
    match probe.authentication {
        crate::execution::contract::ReadinessState::Unavailable => {
            findings.push(Finding::action(
                format!("{prefix}:auth"),
                &format!("Connect {name}"),
                "The provider reports that sign-in or account configuration is required.",
                "Complete the provider's official sign-in flow without sharing credentials with Kool.ad/e, then retry verification.",
            ));
        }
        crate::execution::contract::ReadinessState::Unknown => {
            findings.push(Finding::unverified(
                format!("{prefix}:auth-verify"),
                &format!("Verify {name} access"),
                "The provider does not expose a safe authentication result that Kool.ad/e can verify.",
                "Follow the official sign-in guide. Kool.ad/e will keep access unverified until a supported check confirms it.",
            ));
        }
        crate::execution::contract::ReadinessState::Ready => {}
    }

    if probe.model_catalog_status != ModelCatalogStatus::Fresh
        || (probe.models.is_empty() && probe.default_model.is_none())
    {
        findings.push(Finding::unverified(
            format!("{prefix}:models"),
            &format!("Choose a usable model in {name}"),
            "Kool.ad/e could not verify a current configured model for this provider.",
            "Use the provider's official model setup guide, then refresh discovery. Model names alone do not prove account access.",
        ));
    }

    if let Some(reason) =
        crate::harness::execution_security::task_execution_block_reason(provider_id)
    {
        findings.push(Finding::unverified(
            format!("{prefix}:capability"),
            &format!("Use another provider until {name} is safely sandboxed"),
            reason,
            &format!("Choose another provider for setup assistance. Kool.ad/e will keep {name} available for installation and sign-in guidance, but will not start its task process until managed hooks can be contained."),
        ));
    } else if probe.execution_capability.safe_for_planning
        != crate::execution::contract::ReadinessState::Ready
    {
        findings.push(Finding::unverified(
            format!("{prefix}:capability"),
            &format!("Verify safe setup assistance with {name}"),
            "Kool.ad/e could not verify this provider's context-only invocation mode.",
            "Review the provider's capability details. Host commands and project tools remain unavailable to setup guidance.",
        ));
    }

    findings
}
