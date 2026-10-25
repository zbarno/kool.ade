use super::*;
use crate::app::setup_attention::catalog::{Finding, FindingStatus, HostKind, MachineSnapshot};

mod cancellation;
mod cross_process;
mod ordering;

fn snapshot(at: u64, title: &str) -> MachineSnapshot {
    use crate::app::setup_attention::catalog::{
        EvidenceCode, EvidenceOrigin, EvidenceState, PrerequisiteCapability as Capability,
        PrerequisiteDomain as Domain, PrerequisiteId, PrerequisiteNodeV1,
        PrerequisiteSubject as Subject, ProbeObservationV1, RemediationEligibility,
    };

    let probe_evidence = [
        (
            Capability::HostFamily,
            EvidenceState::Ready,
            EvidenceCode::RuntimeTargetDetected,
        ),
        (
            Capability::HostArchitecture,
            EvidenceState::Ready,
            EvidenceCode::ArchitectureDetected,
        ),
        (
            Capability::LinuxDistribution,
            EvidenceState::Unknown,
            EvidenceCode::HostCapabilityNotProbed,
        ),
        (
            Capability::LinuxUidMappings,
            EvidenceState::Unknown,
            EvidenceCode::HostCapabilityNotProbed,
        ),
        (
            Capability::LinuxServiceManager,
            EvidenceState::Unknown,
            EvidenceCode::LinuxServiceManagerUnknown,
        ),
        (
            Capability::LinuxPackageManager,
            EvidenceState::Unknown,
            EvidenceCode::HostCapabilityNotProbed,
        ),
        (
            Capability::Wsl2,
            EvidenceState::NotApplicable,
            EvidenceCode::HostCapabilityNotProbed,
        ),
        (
            Capability::Virtualization,
            EvidenceState::NotApplicable,
            EvidenceCode::HostCapabilityNotProbed,
        ),
        (
            Capability::DockerDesktopTrust,
            EvidenceState::NotApplicable,
            EvidenceCode::DockerDesktopTrustUnverified,
        ),
    ]
    .into_iter()
    .map(|(capability, state, code)| {
        let observation = match capability {
            Capability::HostArchitecture => Some(ProbeObservationV1::HostArchitecture {
                observed: crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
                supported_runtime_architectures: vec![
                    crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
                ],
            }),
            Capability::LinuxDistribution => Some(ProbeObservationV1::LinuxDistribution {
                distribution: None,
                version: None,
            }),
            _ => None,
        };
        crate::app::setup_attention::catalog::ProbeEvidenceV1 {
            prerequisite: PrerequisiteId {
                domain: Domain::Host,
                platform: HostKind::Linux,
                capability,
                subject: Subject::None,
            },
            state,
            code,
            origin: EvidenceOrigin::HostRuntime,
            scan_generation: at,
            source_generation: None,
            observed_at_unix_ms: at,
            observation,
        }
    })
    .collect::<Vec<_>>();
    let prerequisite_graph = probe_evidence
        .iter()
        .map(|evidence| PrerequisiteNodeV1 {
            id: evidence.prerequisite.clone(),
            state: evidence.state,
            depends_on: Vec::new(),
            remediation: RemediationEligibility::NotRequired,
        })
        .collect();
    MachineSnapshot {
        schema_version: MachineSnapshot::SCHEMA_VERSION,
        scan_generation: at,
        scan_started_at_unix_ms: at,
        host: HostKind::Linux,
        selected_provider: None,
        checked_at_unix_ms: at,
        capability_matrix: crate::app::setup_attention::catalog::HostCapabilityMatrixV1 {
            native_controller: crate::app::setup_attention::catalog::SupportClaim::Unverified,
            observed_host_architecture:
                crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
            supported_runtime_architectures: vec![
                crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64,
            ],
            linux_distribution: crate::app::setup_attention::catalog::EvidenceState::Unknown,
            linux_distribution_id: None,
            linux_distribution_version: None,
            linux_distribution_support: Some(
                crate::app::setup_attention::catalog::SupportClaim::Unverified,
            ),
            linux_uid_mappings: crate::app::setup_attention::catalog::EvidenceState::Unknown,
            linux_service_manager: crate::app::setup_attention::catalog::EvidenceState::Unknown,
            linux_package_manager: crate::app::setup_attention::catalog::EvidenceState::Unknown,
            linux_rootless_mechanism: Some(
                crate::app::setup_attention::catalog::LinuxRootlessMechanismV1::Unknown,
            ),
            windows_wsl2: crate::app::setup_attention::catalog::EvidenceState::NotApplicable,
            virtualization: crate::app::setup_attention::catalog::EvidenceState::NotApplicable,
            docker_desktop_vm_trust:
                crate::app::setup_attention::catalog::EvidenceState::NotApplicable,
        },
        probe_evidence,
        prerequisite_graph,
        findings: vec![Finding {
            id: "setup:docker:rootless".into(),
            title: title.into(),
            explanation: "synthetic concurrent observation".into(),
            next_step: "review".into(),
            status: FindingStatus::NeedsAttention,
            prerequisites: Vec::new(),
        }],
        checked_domains: Vec::new(),
    }
}
