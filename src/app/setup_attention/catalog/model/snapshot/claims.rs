use super::super::evidence::EvidenceState;
use super::super::identifiers::{
    PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
    PrerequisiteSubject as Subject, ProviderIdV1,
};
use super::MachineSnapshotV1;

mod consistency;
mod evidence;
mod images;
mod rootless_smoke;
mod support;
mod windows;

pub(super) use windows::observation_is_consistent as windows_observation_is_consistent;

use images::{image_domain_ready, is_lower_sha256};

fn current_ready(
    snapshot: &MachineSnapshotV1,
    domain: Domain,
    capability: Capability,
    subject: Subject,
    provider_source: bool,
) -> bool {
    snapshot.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.domain == domain
            && evidence.prerequisite.capability == capability
            && evidence.prerequisite.subject == subject
            && evidence.state == EvidenceState::Ready
            && evidence.scan_generation == snapshot.scan_generation
            && evidence::ready_claim(domain, capability, evidence.origin, evidence.code)
            && (provider_source == evidence.source_generation.is_some())
            && (!provider_source || evidence.source_generation == Some(snapshot.scan_generation))
    })
}

fn provider_ready(snapshot: &MachineSnapshotV1, provider: ProviderIdV1) -> bool {
    [
        Capability::ProviderExecutable,
        Capability::ProviderVersion,
        Capability::ProviderAuthentication,
        Capability::ProviderModels,
        Capability::ProviderContextOnly,
    ]
    .into_iter()
    .all(|capability| {
        current_ready(
            snapshot,
            Domain::Provider,
            capability,
            Subject::Provider(provider),
            true,
        )
    })
}

fn provider_scan_current(snapshot: &MachineSnapshotV1, provider: ProviderIdV1) -> bool {
    let items = snapshot
        .probe_evidence
        .iter()
        .filter(|item| {
            item.prerequisite.domain == Domain::Provider
                && item.prerequisite.subject == Subject::Provider(provider)
        })
        .collect::<Vec<_>>();
    items.len() == 5
        && items.iter().all(|item| {
            item.scan_generation == snapshot.scan_generation
                && item.source_generation == Some(snapshot.scan_generation)
                && !matches!(item.state, EvidenceState::Failed | EvidenceState::Stale)
                && evidence::provider_evidence(
                    item.prerequisite.capability,
                    item.state,
                    item.origin,
                    item.code,
                )
        })
        && [
            Capability::ProviderExecutable,
            Capability::ProviderVersion,
            Capability::ProviderAuthentication,
            Capability::ProviderModels,
            Capability::ProviderContextOnly,
        ]
        .into_iter()
        .all(|capability| {
            items
                .iter()
                .any(|item| item.prerequisite.capability == capability)
        })
}

fn docker_ready(snapshot: &MachineSnapshotV1) -> bool {
    snapshot.host == super::super::identifiers::HostKind::Linux
        && [
            Capability::DockerEngine,
            Capability::DockerEndpoint,
            Capability::DockerCgroups,
            Capability::DockerRootless,
            Capability::DockerCapacity,
            Capability::DockerSecurity,
        ]
        .into_iter()
        .all(|capability| current_ready(snapshot, Domain::Docker, capability, Subject::None, false))
}

fn desktop_docker_ready(snapshot: &MachineSnapshotV1) -> bool {
    matches!(
        snapshot.host,
        super::super::identifiers::HostKind::MacOs | super::super::identifiers::HostKind::Windows
    ) && snapshot.capability_matrix.docker_desktop_vm_trust == EvidenceState::Ready
        && snapshot.probe_evidence.iter().any(|evidence| {
            evidence.prerequisite.domain == Domain::Host
                && evidence.prerequisite.platform == snapshot.host
                && evidence.prerequisite.capability == Capability::DockerDesktopTrust
                && evidence.prerequisite.subject == Subject::None
                && evidence.state == EvidenceState::Ready
                && evidence.scan_generation == snapshot.scan_generation
                && evidence.source_generation.is_none()
                && evidence::ready_claim(
                    Domain::Host,
                    Capability::DockerDesktopTrust,
                    evidence.origin,
                    evidence.code,
                )
        })
        && [
            Capability::DockerEngine,
            Capability::DockerEndpoint,
            Capability::DockerDesktopVm,
            Capability::DockerCapacity,
            Capability::DockerSecurity,
        ]
        .into_iter()
        .all(|capability| current_ready(snapshot, Domain::Docker, capability, Subject::None, false))
}

pub(super) fn validate(snapshot: &MachineSnapshotV1) -> anyhow::Result<()> {
    support::validate_capability_matrix(snapshot)?;
    let providers = [
        ProviderIdV1::Pi,
        ProviderIdV1::Codex,
        ProviderIdV1::Claude,
        ProviderIdV1::OpenCode,
        ProviderIdV1::Copilot,
        ProviderIdV1::Antigravity,
    ];
    let mut unique = std::collections::BTreeSet::new();
    for domain in &snapshot.checked_domains {
        anyhow::ensure!(unique.insert(domain), "duplicate checked domain");
        match domain.as_str() {
            "cli:install" => anyhow::ensure!(
                providers.iter().any(|provider| {
                    current_ready(
                        snapshot,
                        Domain::Provider,
                        Capability::ProviderExecutable,
                        Subject::Provider(*provider),
                        true,
                    )
                }),
                "CLI installation domain lacks current positive evidence"
            ),
            "cli:scan" => anyhow::ensure!(
                providers
                    .iter()
                    .all(|provider| provider_scan_current(snapshot, *provider)),
                "CLI scan domain lacks current provider evidence"
            ),
            "cli:select" => anyhow::ensure!(
                snapshot
                    .selected_provider
                    .is_some_and(|provider| provider_ready(snapshot, provider)),
                "CLI selection domain lacks current selected-provider evidence"
            ),
            "git:executable" => anyhow::ensure!(
                current_ready(
                    snapshot,
                    Domain::Git,
                    Capability::GitExecutable,
                    Subject::None,
                    false,
                ),
                "Git executable domain lacks current positive evidence"
            ),
            "git:identity" => anyhow::ensure!(
                [Capability::GitExecutable, Capability::GitIdentity]
                    .into_iter()
                    .all(|capability| {
                        current_ready(snapshot, Domain::Git, capability, Subject::None, false)
                    }),
                "Git identity domain lacks current executable and identity evidence"
            ),
            "git" => anyhow::ensure!(
                [Capability::GitExecutable, Capability::GitIdentity]
                    .into_iter()
                    .all(|capability| {
                        current_ready(snapshot, Domain::Git, capability, Subject::None, false)
                    }),
                "Git readiness domain lacks current positive evidence"
            ),
            "docker" => anyhow::ensure!(
                docker_ready(snapshot),
                "Docker readiness domain lacks current trusted Linux evidence"
            ),
            "docker:desktop" => anyhow::ensure!(
                desktop_docker_ready(snapshot),
                "Docker Desktop readiness domain lacks current positive evidence"
            ),
            "docker:rootless-smoke" => anyhow::ensure!(
                rootless_smoke::ready_for_any_architecture(snapshot),
                "rootless smoke domain lacks a current approved-image smoke receipt"
            ),
            _ if domain.starts_with("cli:") => {
                let provider_id = domain.strip_prefix("cli:").unwrap_or_default();
                let provider = ProviderIdV1::from_probe_id(provider_id);
                anyhow::ensure!(
                    provider.is_some_and(|provider| provider_ready(snapshot, provider)),
                    "provider domain lacks current readiness evidence"
                );
            }
            _ if domain.starts_with("images:") => {
                let parts = domain.split(':').collect::<Vec<_>>();
                anyhow::ensure!(
                    (parts.len() == 3 && image_domain_ready(snapshot, parts[1], parts[2], None))
                        || (parts.len() == 4
                            && is_lower_sha256(parts[3])
                            && image_domain_ready(snapshot, parts[1], parts[2], Some(parts[3]))),
                    "image domain lacks exact current positive evidence"
                );
            }
            _ => anyhow::bail!("unknown checked readiness domain"),
        }
        anyhow::ensure!(
            snapshot
                .findings
                .iter()
                .all(|finding| !consistency::finding_conflicts(domain, &finding.id)),
            "checked readiness domain conflicts with an active setup finding"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
