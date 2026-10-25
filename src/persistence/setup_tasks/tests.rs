use super::*;
use crate::app::setup_attention::catalog::{
    EvidenceCode, EvidenceOrigin, EvidenceState, Finding, FindingStatus, HostCapabilityMatrixV1,
    HostKind, ImageArchitectureV1, ImageProfileV1, MachineSnapshotV1, PrerequisiteCapability,
    PrerequisiteDomain, PrerequisiteId, PrerequisiteNodeV1, PrerequisiteSubject, ProbeEvidenceV1,
    ProbeObservationV1, RemediationEligibility, SupportClaim,
};

mod reconciliation;

fn snapshot(ids: &[(&str, FindingStatus)], domains: &[&str], at: u64) -> MachineSnapshot {
    let mut probe_evidence = Vec::new();
    let mut add = |domain, capability, subject, state, code, origin| {
        let observation = match capability {
            PrerequisiteCapability::HostArchitecture => {
                Some(ProbeObservationV1::HostArchitecture {
                    observed: ImageArchitectureV1::Amd64,
                    supported_runtime_architectures: vec![
                        ImageArchitectureV1::Amd64,
                        ImageArchitectureV1::Arm64,
                    ],
                })
            }
            PrerequisiteCapability::LinuxDistribution => {
                Some(ProbeObservationV1::LinuxDistribution {
                    distribution: None,
                    version: None,
                })
            }
            _ => None,
        };
        probe_evidence.push(ProbeEvidenceV1 {
            prerequisite: PrerequisiteId {
                domain,
                platform: HostKind::Linux,
                capability,
                subject,
            },
            state,
            code,
            origin,
            scan_generation: at,
            source_generation: None,
            observed_at_unix_ms: at,
            observation,
        });
    };
    for (capability, state, code) in [
        (
            PrerequisiteCapability::HostFamily,
            EvidenceState::Ready,
            EvidenceCode::RuntimeTargetDetected,
        ),
        (
            PrerequisiteCapability::HostEdition,
            EvidenceState::NotApplicable,
            EvidenceCode::HostEditionNotProbed,
        ),
        (
            PrerequisiteCapability::HostArchitecture,
            EvidenceState::Ready,
            EvidenceCode::ArchitectureDetected,
        ),
        (
            PrerequisiteCapability::LinuxDistribution,
            EvidenceState::Unknown,
            EvidenceCode::HostCapabilityNotProbed,
        ),
        (
            PrerequisiteCapability::LinuxUidMappings,
            EvidenceState::Unknown,
            EvidenceCode::HostCapabilityNotProbed,
        ),
        (
            PrerequisiteCapability::LinuxServiceManager,
            EvidenceState::Unknown,
            EvidenceCode::LinuxServiceManagerUnknown,
        ),
        (
            PrerequisiteCapability::LinuxPackageManager,
            EvidenceState::Unknown,
            EvidenceCode::HostCapabilityNotProbed,
        ),
        (
            PrerequisiteCapability::Wsl2,
            EvidenceState::NotApplicable,
            EvidenceCode::HostCapabilityNotProbed,
        ),
        (
            PrerequisiteCapability::Virtualization,
            EvidenceState::NotApplicable,
            EvidenceCode::HostCapabilityNotProbed,
        ),
        (
            PrerequisiteCapability::DockerDesktopTrust,
            EvidenceState::NotApplicable,
            EvidenceCode::DockerDesktopTrustUnverified,
        ),
    ] {
        add(
            PrerequisiteDomain::Host,
            capability,
            PrerequisiteSubject::None,
            state,
            code,
            EvidenceOrigin::HostRuntime,
        );
    }
    if domains.contains(&"docker") || domains.iter().any(|domain| domain.starts_with("images:")) {
        for capability in [
            PrerequisiteCapability::DockerEngine,
            PrerequisiteCapability::DockerEndpoint,
            PrerequisiteCapability::DockerCgroups,
            PrerequisiteCapability::DockerRootless,
            PrerequisiteCapability::DockerCapacity,
            PrerequisiteCapability::DockerSecurity,
        ] {
            let code = if capability == PrerequisiteCapability::DockerCgroups {
                EvidenceCode::DockerCgroupsSupported
            } else {
                EvidenceCode::DockerReady
            };
            add(
                PrerequisiteDomain::Docker,
                capability,
                PrerequisiteSubject::None,
                EvidenceState::Ready,
                code,
                EvidenceOrigin::DockerDiscovery,
            );
        }
    }
    if domains.contains(&"git") || domains.contains(&"git:executable") {
        add(
            PrerequisiteDomain::Git,
            PrerequisiteCapability::GitExecutable,
            PrerequisiteSubject::None,
            EvidenceState::Ready,
            EvidenceCode::GitAvailable,
            EvidenceOrigin::GitCli,
        );
    }
    if domains.contains(&"git") || domains.contains(&"git:identity") {
        add(
            PrerequisiteDomain::Git,
            PrerequisiteCapability::GitIdentity,
            PrerequisiteSubject::None,
            EvidenceState::Ready,
            EvidenceCode::GitIdentityConfigured,
            EvidenceOrigin::GitCli,
        );
    }
    for domain in domains
        .iter()
        .filter(|domain| domain.starts_with("images:"))
    {
        let parts = domain.split(':').collect::<Vec<_>>();
        if let ["images", profile, architecture] = parts.as_slice()
            && let (Some(profile), Some(architecture)) =
                (image_profile(profile), image_architecture(architecture))
        {
            add(
                PrerequisiteDomain::Image,
                PrerequisiteCapability::ApprovedImage,
                PrerequisiteSubject::Image {
                    profile,
                    architecture,
                },
                EvidenceState::Ready,
                EvidenceCode::ImagePresent,
                EvidenceOrigin::ImageInventory,
            );
        }
    }
    let prerequisite_graph = probe_evidence
        .iter()
        .map(|evidence| PrerequisiteNodeV1 {
            id: evidence.prerequisite.clone(),
            state: evidence.state,
            depends_on: Vec::new(),
            remediation: RemediationEligibility::NotRequired,
        })
        .collect();
    MachineSnapshotV1 {
        schema_version: SNAPSHOT_VERSION,
        scan_generation: at,
        scan_started_at_unix_ms: at,
        host: HostKind::Linux,
        selected_provider: None,
        checked_at_unix_ms: at,
        capability_matrix: HostCapabilityMatrixV1 {
            native_controller: SupportClaim::Unverified,
            observed_host_architecture: ImageArchitectureV1::Amd64,
            supported_runtime_architectures: vec![
                ImageArchitectureV1::Amd64,
                ImageArchitectureV1::Arm64,
            ],
            linux_distribution: EvidenceState::Unknown,
            linux_distribution_id: None,
            linux_distribution_version: None,
            linux_distribution_support: Some(SupportClaim::Unverified),
            linux_uid_mappings: EvidenceState::Unknown,
            linux_service_manager: EvidenceState::Unknown,
            linux_package_manager: EvidenceState::Unknown,
            linux_rootless_mechanism: Some(
                crate::app::setup_attention::catalog::LinuxRootlessMechanismV1::Unknown,
            ),
            windows_wsl2: EvidenceState::NotApplicable,
            virtualization: EvidenceState::NotApplicable,
            docker_desktop_vm_trust: EvidenceState::NotApplicable,
        },
        probe_evidence,
        prerequisite_graph,
        findings: ids
            .iter()
            .map(|(id, status)| Finding {
                id: (*id).into(),
                title: (*id).into(),
                explanation: "safe observation".into(),
                next_step: "retry".into(),
                status: *status,
                prerequisites: Vec::new(),
            })
            .collect(),
        checked_domains: domains.iter().map(|s| (*s).into()).collect(),
    }
}

fn image_profile(value: &str) -> Option<ImageProfileV1> {
    match value {
        "general" => Some(ImageProfileV1::General),
        "rust" => Some(ImageProfileV1::Rust),
        "node" => Some(ImageProfileV1::Node),
        "python" => Some(ImageProfileV1::Python),
        "go" => Some(ImageProfileV1::Go),
        _ => None,
    }
}

fn image_architecture(value: &str) -> Option<ImageArchitectureV1> {
    match value {
        "amd64" => Some(ImageArchitectureV1::Amd64),
        "arm64" => Some(ImageArchitectureV1::Arm64),
        _ => None,
    }
}
