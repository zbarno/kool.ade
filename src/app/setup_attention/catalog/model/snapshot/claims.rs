use super::super::evidence::EvidenceState;
use super::super::identifiers::{
    PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
    PrerequisiteSubject as Subject, ProviderIdV1,
};
use super::MachineSnapshotV1;

mod consistency;
mod evidence;
mod images;
mod providers;
mod rootless_smoke;
mod support;

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
    ) && [
        Capability::DockerEngine,
        Capability::DockerEndpoint,
        Capability::DockerDesktopVm,
    ]
    .into_iter()
    .all(|capability| current_ready(snapshot, Domain::Docker, capability, Subject::None, false))
}

pub(super) fn validate(snapshot: &MachineSnapshotV1) -> anyhow::Result<()> {
    support::validate_capability_matrix(snapshot)?;
    if let Some(provider) = snapshot.setup_provider {
        anyhow::ensure!(
            providers::provider_scan_current(snapshot, provider)
                || providers::missing_setup_provider_has_verification_finding(snapshot, provider),
            "selected setup provider lacks current provider scan evidence or an explicit verification finding"
        );
    }
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
                providers::explicit_provider_choice_current(snapshot),
                "CLI provider selection domain lacks a current explicit provider choice"
            ),
            "cli:scan" => anyhow::ensure!(
                providers
                    .iter()
                    .all(|provider| providers::provider_scan_current(snapshot, *provider)),
                "CLI scan domain lacks current provider evidence"
            ),
            "cli:select" => anyhow::ensure!(
                snapshot
                    .selected_provider
                    .is_some_and(|provider| providers::provider_ready(snapshot, provider)),
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
                    provider.is_some_and(|provider| providers::provider_ready(snapshot, provider)),
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
