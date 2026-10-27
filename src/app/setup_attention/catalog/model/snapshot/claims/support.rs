use crate::app::setup_attention::catalog::{
    EvidenceCode, EvidenceOrigin, EvidenceState, HostKind, ImageArchitectureV1,
    LinuxDistributionV1, LinuxDistributionVersionV1, LinuxRootlessMechanismV1,
    LinuxRootlessRecipeV1, PrerequisiteCapability as Capability, PrerequisiteDomain as Domain,
    PrerequisiteSubject as Subject, ProbeEvidenceV1, ProbeObservationV1 as Observation,
    SupportClaim,
};

use super::super::MachineSnapshotV1;

mod linux_probes;
mod windows_probes;

pub(super) fn validate_capability_matrix(snapshot: &MachineSnapshotV1) -> anyhow::Result<()> {
    linux_probes::validate(snapshot)?;
    windows_probes::validate(snapshot)?;
    let matrix = &snapshot.capability_matrix;
    validate_host_fact(snapshot, Capability::HostFamily, EvidenceState::Ready)?;
    let architecture_state = match matrix.observed_host_architecture {
        ImageArchitectureV1::Amd64 | ImageArchitectureV1::Arm64 => EvidenceState::Ready,
        ImageArchitectureV1::Unsupported => EvidenceState::Unsupported,
        ImageArchitectureV1::Unknown => EvidenceState::Unknown,
    };
    let architecture = host_evidence(snapshot, Capability::HostArchitecture)?;
    validate_host_fact(snapshot, Capability::HostArchitecture, architecture_state)?;
    let Some(Observation::HostArchitecture {
        observed,
        supported_runtime_architectures,
    }) = &architecture.observation
    else {
        anyhow::bail!("host architecture evidence lacks its typed observation");
    };
    anyhow::ensure!(
        *observed == matrix.observed_host_architecture
            && supported_runtime_architectures == &matrix.supported_runtime_architectures,
        "host architecture matrix conflicts with typed probe evidence"
    );
    anyhow::ensure!(
        matrix.supported_runtime_architectures.len() <= 2,
        "supported runtime architecture list is oversized"
    );
    let runtime_architectures = matrix
        .supported_runtime_architectures
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    anyhow::ensure!(
        runtime_architectures.len() == matrix.supported_runtime_architectures.len()
            && matrix
                .supported_runtime_architectures
                .iter()
                .all(|architecture| {
                    matches!(
                        architecture,
                        ImageArchitectureV1::Amd64 | ImageArchitectureV1::Arm64
                    )
                }),
        "supported runtime architecture list is invalid"
    );
    let distribution = host_evidence(snapshot, Capability::LinuxDistribution)?;
    validate_host_fact(
        snapshot,
        Capability::LinuxDistribution,
        matrix.linux_distribution,
    )?;
    let Some(Observation::LinuxDistribution {
        distribution: observed_distribution,
        version: observed_version,
    }) = &distribution.observation
    else {
        anyhow::bail!("Linux distribution evidence lacks its typed observation");
    };
    anyhow::ensure!(
        *observed_distribution == matrix.linux_distribution_id
            && *observed_version == matrix.linux_distribution_version,
        "Linux distribution matrix conflicts with typed probe evidence"
    );
    validate_host_fact(
        snapshot,
        Capability::LinuxUidMappings,
        matrix.linux_uid_mappings,
    )?;
    validate_host_fact(
        snapshot,
        Capability::LinuxServiceManager,
        matrix.linux_service_manager,
    )?;
    validate_host_fact(
        snapshot,
        Capability::LinuxPackageManager,
        matrix.linux_package_manager,
    )?;
    validate_host_fact(snapshot, Capability::Wsl2, matrix.windows_wsl2)?;
    validate_host_fact(snapshot, Capability::Virtualization, matrix.virtualization)?;
    validate_host_fact(
        snapshot,
        Capability::DockerDesktopTrust,
        matrix.docker_desktop_vm_trust,
    )?;

    if snapshot.host == HostKind::Linux {
        anyhow::ensure!(
            matrix.linux_distribution_support.is_some()
                && matrix.linux_rootless_mechanism.is_some(),
            "Linux support matrix lacks distribution or rootless mechanism evidence"
        );
        anyhow::ensure!(
            matrix.linux_distribution_version.is_none() || matrix.linux_distribution_id.is_some(),
            "Linux distribution version lacks a typed distribution ID"
        );
        anyhow::ensure!(
            (matrix.linux_distribution_id.is_some()
                && matrix.linux_distribution == EvidenceState::Ready)
                || (matrix.linux_distribution_id.is_none()
                    && matrix.linux_distribution == EvidenceState::Unknown),
            "Linux distribution evidence contradicts its typed identity"
        );
        anyhow::ensure!(
            (matrix.linux_rootless_mechanism == Some(LinuxRootlessMechanismV1::SystemdUser))
                == (matrix.linux_service_manager == EvidenceState::Ready),
            "Linux rootless mechanism conflicts with service manager evidence"
        );
        anyhow::ensure!(
            matrix.linux_distribution_support
                == Some(distribution_support(
                    matrix.linux_distribution_id,
                    matrix.linux_distribution_version,
                )),
            "Linux distribution support claim conflicts with observed version"
        );
    } else {
        anyhow::ensure!(
            matrix.linux_distribution == EvidenceState::NotApplicable
                && matrix.linux_distribution_id.is_none()
                && matrix.linux_distribution_version.is_none()
                && matrix.linux_distribution_support.is_none()
                && matrix.linux_uid_mappings == EvidenceState::NotApplicable
                && matrix.linux_service_manager == EvidenceState::NotApplicable
                && matrix.linux_package_manager == EvidenceState::NotApplicable
                && matrix.linux_rootless_mechanism.is_none(),
            "non-Linux support matrix contains Linux-only claims"
        );
    }
    let expected_native_support = match snapshot.host {
        HostKind::Linux
            if LinuxRootlessRecipeV1::for_host(
                matrix.linux_distribution_id,
                matrix.linux_distribution_version,
                matrix.observed_host_architecture,
            )
            .is_some() =>
        {
            SupportClaim::Experimental
        }
        HostKind::Linux => SupportClaim::Unverified,
        HostKind::MacOs | HostKind::Windows => SupportClaim::Planned,
        HostKind::Unsupported => SupportClaim::Unsupported,
    };
    anyhow::ensure!(
        matrix.native_controller == expected_native_support,
        "native controller support claim conflicts with platform evidence"
    );
    Ok(())
}

fn validate_host_fact(
    snapshot: &MachineSnapshotV1,
    capability: Capability,
    expected: EvidenceState,
) -> anyhow::Result<()> {
    let evidence = host_evidence(snapshot, capability)?;
    let expected_code = host_evidence_code(capability, expected);
    let code_matches = evidence.code == expected_code
        || (expected == EvidenceState::Unknown
            && matches!(
                capability,
                Capability::LinuxUidMappings
                    | Capability::LinuxServiceManager
                    | Capability::LinuxPackageManager
            )
            && evidence.code == EvidenceCode::LinuxProbeInaccessible)
        || (!linux_probes::has_new_facts(snapshot)
            && expected == EvidenceState::Unknown
            && match capability {
                Capability::LinuxUidMappings => {
                    evidence.code == EvidenceCode::HostCapabilityNotProbed
                }
                Capability::LinuxServiceManager => {
                    evidence.code == EvidenceCode::LinuxServiceManagerUnknown
                }
                _ => false,
            });
    anyhow::ensure!(
        evidence.scan_generation == snapshot.scan_generation
            && evidence.source_generation.is_none()
            && evidence.origin == EvidenceOrigin::HostRuntime
            && evidence.state == expected
            && code_matches,
        "host capability matrix conflicts with current probe evidence"
    );
    Ok(())
}

fn host_evidence(
    snapshot: &MachineSnapshotV1,
    capability: Capability,
) -> anyhow::Result<&ProbeEvidenceV1> {
    snapshot
        .probe_evidence
        .iter()
        .find(|item| {
            item.prerequisite.domain == Domain::Host
                && item.prerequisite.platform == snapshot.host
                && item.prerequisite.capability == capability
                && item.prerequisite.subject == Subject::None
        })
        .ok_or_else(|| anyhow::anyhow!("host capability matrix lacks matching probe evidence"))
}

fn host_evidence_code(capability: Capability, state: EvidenceState) -> EvidenceCode {
    match capability {
        Capability::HostFamily => EvidenceCode::RuntimeTargetDetected,
        Capability::HostEdition => {
            if state == EvidenceState::Ready {
                EvidenceCode::MacOsReleaseDetected
            } else {
                EvidenceCode::HostEditionNotProbed
            }
        }
        Capability::HostArchitecture => EvidenceCode::ArchitectureDetected,
        Capability::LinuxDistribution => {
            if state == EvidenceState::Ready {
                EvidenceCode::LinuxDistributionDetected
            } else {
                EvidenceCode::HostCapabilityNotProbed
            }
        }
        Capability::LinuxUidMappings => match state {
            EvidenceState::Ready => EvidenceCode::LinuxUidMappingsConfigured,
            EvidenceState::Missing => EvidenceCode::LinuxUidMappingsMissing,
            EvidenceState::Failed => EvidenceCode::LinuxProbeMalformed,
            EvidenceState::Unknown => EvidenceCode::LinuxProbeInaccessible,
            _ => EvidenceCode::HostCapabilityNotProbed,
        },
        Capability::LinuxServiceManager => match state {
            EvidenceState::Ready => EvidenceCode::LinuxServiceManagerDetected,
            EvidenceState::Missing => EvidenceCode::LinuxHostCheckMissing,
            EvidenceState::Failed => EvidenceCode::LinuxProbeMalformed,
            EvidenceState::Unknown => EvidenceCode::HostCapabilityNotProbed,
            EvidenceState::Unsupported => EvidenceCode::LinuxProbeUnsupported,
            _ => EvidenceCode::HostCapabilityNotProbed,
        },
        Capability::LinuxPackageManager => match state {
            EvidenceState::Ready => EvidenceCode::LinuxPackageManagerDetected,
            EvidenceState::Missing => EvidenceCode::LinuxPackageManagerMissing,
            EvidenceState::Failed => EvidenceCode::LinuxProbeMalformed,
            EvidenceState::Unknown => EvidenceCode::HostCapabilityNotProbed,
            _ => EvidenceCode::HostCapabilityNotProbed,
        },
        Capability::Wsl2 | Capability::Virtualization => EvidenceCode::HostCapabilityNotProbed,
        Capability::DockerDesktopTrust => {
            if state == EvidenceState::Ready {
                EvidenceCode::DockerDesktopTrustVerified
            } else {
                EvidenceCode::DockerDesktopTrustUnverified
            }
        }
        _ => EvidenceCode::HostCapabilityNotProbed,
    }
}

fn distribution_support(
    distribution: Option<LinuxDistributionV1>,
    version: Option<LinuxDistributionVersionV1>,
) -> SupportClaim {
    if LinuxRootlessRecipeV1::for_host(distribution, version, ImageArchitectureV1::Amd64).is_some()
    {
        SupportClaim::Experimental
    } else {
        SupportClaim::Unverified
    }
}
