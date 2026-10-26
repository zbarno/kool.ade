use crate::execution::docker::environment::DockerEnvironmentStatus;
use crate::harness::discovery::{ProbeResult, ProbeStatus};

use super::{
    EvidenceCode, EvidenceState, Finding, FindingStatus, GitProbeV1, HostKind, MachineSnapshotV1,
    ProviderChoices, ProviderIdV1, SCHEMA_VERSION, evidence, images,
};

mod checked_domains;
mod docker;
mod inputs;
mod provider_tasks;
pub(crate) use inputs::{ImageScanState, ScanInputs};
#[cfg(test)]
mod test_support;

#[cfg(test)]
pub use test_support::{assess, assess_for_scan, assess_for_scan_with_git};

/// Pure, deterministic interpretation. Missing image findings require
/// engine_ready; a rootful or unqueryable engine is NOT an image failure.
#[cfg(test)]
pub fn assess_for_scan_with_git_and_bootstrap_provider(
    probes: &[ProbeResult],
    provider_choices: ProviderChoices<'_>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    checked_at_unix_ms: u64,
    scan_generation: u64,
    git_probe: GitProbeV1,
) -> MachineSnapshotV1 {
    assess_for_scan_with_inputs_and_bootstrap_provider(
        probes,
        provider_choices,
        docker,
        host,
        checked_at_unix_ms,
        scan_generation,
        ScanInputs {
            git_probe,
            ..ScanInputs::default()
        },
    )
}

pub(crate) fn assess_for_scan_with_inputs_and_bootstrap_provider(
    probes: &[ProbeResult],
    provider_choices: ProviderChoices<'_>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    checked_at_unix_ms: u64,
    scan_generation: u64,
    inputs: ScanInputs,
) -> MachineSnapshotV1 {
    let ScanInputs {
        git_probe,
        images: image_state,
    } = inputs;
    let mut findings = Vec::new();
    let verified: Vec<_> = probes
        .iter()
        .filter(|p| provider_tasks::authenticated_context_agent(p, scan_generation))
        .collect();

    let explicit_provider_ids = [
        provider_choices.guided_setup,
        provider_choices.default_agent,
    ]
    .into_iter()
    .flatten()
    .fold(Vec::new(), |mut ids, id| {
        if !ids.contains(&id) {
            ids.push(id);
        }
        ids
    });
    if !explicit_provider_ids.is_empty() {
        for provider_id in &explicit_provider_ids {
            findings.extend(provider_tasks::selected_provider_findings(
                provider_id,
                probes,
                scan_generation,
            ));
        }
    } else {
        if !probes.is_empty()
            && probes
                .iter()
                .all(|probe| probe.status == ProbeStatus::Failed)
        {
            findings.push(Finding::unverified(
                "setup:cli:scan",
                "Retry coding agent discovery",
                "Kool.ad/e could not complete the provider checks on this machine.",
                "Retry discovery in Coding Tools before choosing a provider.",
            ));
        } else {
            findings.push(provider_tasks::choose_provider());
        }
    }
    if !verified.is_empty() && provider_choices.default_agent.is_none() {
        findings.push(Finding::action(
            "setup:cli:select",
            "Choose a default coding agent",
            "At least one provider is ready, but no default coding agent was selected.",
            "Open Coding Tools and choose Default tool on the provider you want to route coding work to.",
        ));
    }

    if let Some(finding) = docker::finding(docker) {
        findings.push(finding);
    }

    findings.extend(images::findings(
        docker,
        host,
        &image_state.required_profiles,
        &image_state.smoke_verified_profiles,
    ));

    // Docker Desktop endpoints are parseable today, but safe end-to-end
    // execution on these hosts is not production validated (#347).
    if matches!(host, HostKind::MacOs | HostKind::Windows) {
        findings.push(Finding {
            status: FindingStatus::Unsupported,
            ..Finding::unverified(
                "setup:docker:desktop-transport",
                "Verify Docker Desktop compatibility",
                "Docker Desktop execution on this host is not yet release-certified.",
                "Complete native Docker Desktop staging/security validation before enabling coding tasks.",
            )
        });
    } else if host == HostKind::Unsupported {
        findings.push(Finding {
            status: FindingStatus::Unsupported,
            ..Finding::unverified(
                "setup:host:unsupported",
                "Unsupported operating system",
                "Application-owned Docker execution has not been validated on this host.",
                "Use a supported operating system for coding tasks.",
            )
        });
    }

    let (capability_matrix, probe_evidence, prerequisite_graph) = evidence::collect(
        probes,
        docker,
        host,
        scan_generation,
        checked_at_unix_ms,
        git_probe,
    );
    match git_probe.executable_state {
        EvidenceState::Missing => findings.push(Finding::action(
            "setup:git:install",
            "Install Git",
            "Git is required to manage and safely inspect project repositories.",
            "Install Git using the official instructions for this operating system, then retry.",
        )),
        EvidenceState::Failed | EvidenceState::Unknown | EvidenceState::Stale
            if git_probe.executable_code != EvidenceCode::GitProbeNotRun =>
        {
            findings.push(Finding::unverified(
                "setup:git:verify",
                "Verify Git availability",
                "Kool.ad/e could not verify the Git command needed for project repositories.",
                "Check Git availability and retry the machine setup scan.",
            ));
        }
        _ => {}
    }
    match git_probe.identity_state {
        EvidenceState::Missing => findings.push(Finding::action(
            "setup:git:identity",
            "Configure a Git author identity",
            "Git needs an author name and email before it can create project commits.",
            "Configure user.name and user.email in Git, then retry the setup scan.",
        )),
        EvidenceState::Failed | EvidenceState::Unknown | EvidenceState::Stale
            if git_probe.identity_code != EvidenceCode::ParentPrerequisiteUnavailable =>
        {
            findings.push(Finding::unverified(
                "setup:git:verify",
                "Verify Git identity settings",
                "Kool.ad/e could not verify the Git author identity configuration.",
                "Check the global Git identity settings, then retry the setup scan.",
            ));
        }
        _ => {}
    }
    findings.sort_by(|left, right| left.id.cmp(&right.id));
    findings.dedup_by(|left, right| left.id == right.id);
    let snapshot = MachineSnapshotV1 {
        schema_version: SCHEMA_VERSION,
        scan_generation,
        // Pure callers may supply pre-recorded probes. Bound their evidence
        // interval here; the live scanner replaces this with its actual
        // before-probes start time before persistence.
        scan_started_at_unix_ms: probe_evidence
            .iter()
            .map(|item| item.observed_at_unix_ms)
            .min()
            .unwrap_or(checked_at_unix_ms),
        host,
        setup_provider: provider_choices
            .guided_setup
            .and_then(ProviderIdV1::from_probe_id),
        selected_provider: provider_choices
            .default_agent
            .and_then(ProviderIdV1::from_probe_id),
        checked_at_unix_ms,
        capability_matrix,
        probe_evidence,
        prerequisite_graph,
        findings,
        checked_domains: checked_domains::collect(checked_domains::Inputs {
            probes,
            scan_generation,
            verified: &verified,
            setup_provider: provider_choices.guided_setup,
            selected_agent: provider_choices.default_agent,
            docker,
            host,
            git_probe: &git_probe,
            image_state: &image_state,
        }),
    };
    debug_assert!(
        snapshot.validate().is_ok(),
        "invalid machine readiness snapshot: {:?}",
        snapshot.validate()
    );
    snapshot
}

pub(crate) fn probe_git() -> GitProbeV1 {
    evidence::probe_git()
}
