use crate::execution::{contract::ReadinessState, docker::environment::DockerEnvironmentStatus};
use crate::harness::discovery::{ModelCatalogStatus, ProbeResult, ProbeStatus};

use super::{
    EvidenceCode, EvidenceState, Finding, FindingStatus, GitProbeV1, HostKind, MachineSnapshotV1,
    ProviderIdV1, SCHEMA_VERSION, evidence, images,
};

mod docker;

fn authenticated_context_agent(probe: &ProbeResult, generation: u64) -> bool {
    probe.status == ProbeStatus::Available
        && probe.run_generation == generation
        && probe.binary_path.is_some()
        && probe.version.is_some()
        && probe.authentication == ReadinessState::Ready
        && probe.execution_capability.safe_for_planning == ReadinessState::Ready
        && probe.model_catalog_status == ModelCatalogStatus::Fresh
        && (!probe.models.is_empty() || probe.default_model.is_some())
}

/// Pure, deterministic interpretation. Missing image findings require
/// engine_ready; a rootful or unqueryable engine is NOT an image failure.
#[cfg(test)]
pub fn assess(
    probes: &[ProbeResult],
    selected_agent: Option<&str>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    checked_at_unix_ms: u64,
) -> MachineSnapshotV1 {
    assess_for_scan(probes, selected_agent, docker, host, checked_at_unix_ms, 0)
}

#[cfg(test)]
pub fn assess_for_scan(
    probes: &[ProbeResult],
    selected_agent: Option<&str>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    checked_at_unix_ms: u64,
    scan_generation: u64,
) -> MachineSnapshotV1 {
    assess_for_scan_with_git(
        probes,
        selected_agent,
        docker,
        host,
        checked_at_unix_ms,
        scan_generation,
        GitProbeV1::not_run(),
    )
}

pub fn assess_for_scan_with_git(
    probes: &[ProbeResult],
    selected_agent: Option<&str>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    checked_at_unix_ms: u64,
    scan_generation: u64,
    git_probe: GitProbeV1,
) -> MachineSnapshotV1 {
    let mut findings = Vec::new();
    let installed: Vec<_> = probes.iter().filter(|p| p.binary_path.is_some()).collect();
    let verified: Vec<_> = installed
        .iter()
        .copied()
        .filter(|p| authenticated_context_agent(p, scan_generation))
        .collect();

    if installed.is_empty() {
        let crashed = probes.iter().any(|p| p.status == ProbeStatus::Failed);
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
        // Only the chosen provider (or installed candidates before selection)
        // should create configuration cards, not every absent optional CLI.
        for probe in installed {
            let prefix = format!("setup:cli:{}", probe.provider_id);
            if probe.run_generation != scan_generation {
                findings.push(Finding::unverified(
                    format!("{prefix}:verify"),
                    "Refresh coding agent evidence",
                    "This coding agent result belongs to an earlier machine scan.",
                    "Retry Coding Tools discovery before relying on this agent.",
                ));
            } else if probe.authentication == ReadinessState::Unavailable {
                findings.push(Finding::action(
                    format!("{prefix}:auth"),
                    "Connect your coding agent account",
                    "The installed coding agent needs an authenticated account.",
                    "Sign in using the agent's official account flow and retry verification.",
                ));
            } else if probe.authentication == ReadinessState::Unknown {
                findings.push(Finding::unverified(
                    format!("{prefix}:verify"),
                    "Verify coding agent access",
                    "This coding agent is installed, but its sign-in status could not be verified.",
                    "Review this agent in Coding Tools; unknown access is not treated as ready.",
                ));
            } else if probe.model_catalog_status != ModelCatalogStatus::Fresh
                || (probe.models.is_empty() && probe.default_model.is_none())
            {
                findings.push(Finding::unverified(
                    format!("{prefix}:models"),
                    "Configure a usable AI model",
                    "This agent's current model availability is not verified.",
                    "Select a supported model and refresh this agent's model catalog.",
                ));
            } else if !authenticated_context_agent(probe, scan_generation) {
                findings.push(Finding::unverified(
                    format!("{prefix}:capability"),
                    "Verify safe agent mode",
                    "The coding agent cannot yet be verified for context-only setup assistance.",
                    "Review its capability diagnostics; host tools remain disabled.",
                ));
            }
        }
        if !verified.is_empty()
            && !selected_agent
                .is_some_and(|id| verified.iter().any(|probe| probe.provider_id == id))
        {
            findings.push(Finding::action(
                "setup:cli:select",
                "Choose your setup assistant",
                "An agent is ready, but none has been explicitly selected.",
                "Select an authenticated agent in Coding Tools.",
            ));
        }
    }

    if let Some(finding) = docker::finding(docker) {
        findings.push(finding);
    }

    findings.extend(images::findings(docker, host));

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
        selected_provider: selected_agent.and_then(ProviderIdV1::from_probe_id),
        checked_at_unix_ms,
        capability_matrix,
        probe_evidence,
        prerequisite_graph,
        findings,
        checked_domains: {
            // Readiness, not merely completed probing, can resolve old tasks.
            // A stopped/rootful/untrusted Engine cannot prove that an earlier
            // Docker prerequisite was fixed. Likewise, an unknown CLI account
            // cannot verify a previously failing provider.
            let mut domains = Vec::new();
            if probes
                .iter()
                .any(|probe| probe.run_generation == scan_generation && probe.binary_path.is_some())
            {
                domains.push("cli:install".into());
            }
            if probes.len() == 6
                && probes.iter().all(|probe| {
                    probe.run_generation == scan_generation && probe.status != ProbeStatus::Failed
                })
            {
                domains.push("cli:scan".into());
            }
            for probe in &verified {
                domains.push(format!("cli:{}", probe.provider_id));
            }
            if selected_agent.is_some_and(|id| verified.iter().any(|probe| probe.provider_id == id))
            {
                domains.push("cli:select".into());
            }
            if host == HostKind::Linux
                && !docker.desktop_vm
                && docker.engine_ready
                && docker.rootless == Some(true)
            {
                domains.push("docker".into());
            }
            if matches!(host, HostKind::MacOs | HostKind::Windows)
                && docker.desktop_vm
                && docker.engine_ready
                && matches!(
                    docker.state,
                    crate::execution::docker::environment::DockerEnvironmentState::MissingApprovedImage
                        | crate::execution::docker::environment::DockerEnvironmentState::Ready
                )
            {
                domains.push("docker:desktop".into());
            }
            if git_probe.executable_state == EvidenceState::Ready {
                domains.push("git:executable".into());
            }
            if git_probe.executable_state == EvidenceState::Ready
                && git_probe.identity_state == EvidenceState::Ready
            {
                domains.push("git:identity".into());
            }
            if git_probe.executable_state == EvidenceState::Ready
                && git_probe.identity_state == EvidenceState::Ready
            {
                domains.push("git".into());
            }
            domains.extend(images::checked_domains(docker, host));
            domains
        },
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
