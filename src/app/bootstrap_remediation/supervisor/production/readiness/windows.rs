use sha2::{Digest, Sha256};

use crate::app::{
    bootstrap_remediation::{
        plan::RemediationPlan,
        supervisor::{
            PreconditionReceipt, PreconditionVerifier, ReadinessVerifier, VerificationReceipt,
        },
    },
    bootstrap_repair_policy::Host,
    setup_attention::{
        self, SetupScanResult,
        catalog::{
            EvidenceState, HostKind, MachineSnapshotV1, PrerequisiteCapability, PrerequisiteDomain,
        },
    },
};

const PRECONDITIONS: &[&str] = &[
    "host:windows:edition",
    "host:windows:architecture",
    "host:windows:virtualization",
    "host:windows:wsl_policy",
    "host:windows:wsl_feature",
    "host:windows:vm_platform_feature",
    "host:windows:reboot",
];
const VERIFICATIONS: &[&str] = &[
    "host:windows:wsl_feature",
    "host:windows:vm_platform_feature",
    "host:windows:reboot",
];

pub(crate) struct WindowsReadinessVerifier;

impl PreconditionVerifier for WindowsReadinessVerifier {
    fn verify(&self, probe_ids: &[String]) -> anyhow::Result<PreconditionReceipt> {
        anyhow::ensure!(
            probe_ids
                .iter()
                .map(String::as_str)
                .eq(PRECONDITIONS.iter().copied()),
            "unsupported or incomplete Windows setup precondition probes"
        );
        let snapshot = collect(probe_ids)?;
        let state = |capability| evidence_state(&snapshot, capability);
        let ready = state(PrerequisiteCapability::HostEdition) == Some(EvidenceState::Ready)
            && state(PrerequisiteCapability::HostArchitecture) == Some(EvidenceState::Ready)
            && state(PrerequisiteCapability::Virtualization) == Some(EvidenceState::Ready)
            && state(PrerequisiteCapability::WindowsWslPolicy) == Some(EvidenceState::Ready)
            && state(PrerequisiteCapability::WindowsReboot) == Some(EvidenceState::Ready)
            && [
                PrerequisiteCapability::WindowsWslFeature,
                PrerequisiteCapability::WindowsVmPlatformFeature,
            ]
            .into_iter()
            .all(|capability| {
                matches!(
                    state(capability),
                    Some(EvidenceState::Ready | EvidenceState::Missing)
                )
            });
        Ok(PreconditionReceipt {
            probe_ids: probe_ids.to_vec(),
            observed_at_unix_ms: snapshot.checked_at_unix_ms,
            evidence_digest: digest(&snapshot, probe_ids)?,
            ready,
        })
    }
}

impl ReadinessVerifier for WindowsReadinessVerifier {
    fn verify(&self, plan: &RemediationPlan) -> anyhow::Result<VerificationReceipt> {
        anyhow::ensure!(
            plan.host == Host::Windows,
            "Windows readiness cannot verify another host"
        );
        anyhow::ensure!(
            plan.verification_probe_ids
                .iter()
                .map(String::as_str)
                .eq(VERIFICATIONS.iter().copied()),
            "unsupported or incomplete Windows setup verification probes"
        );
        let snapshot = collect(&plan.verification_probe_ids)?;
        let ready = [
            PrerequisiteCapability::WindowsWslFeature,
            PrerequisiteCapability::WindowsVmPlatformFeature,
            PrerequisiteCapability::WindowsReboot,
        ]
        .into_iter()
        .all(|capability| evidence_state(&snapshot, capability) == Some(EvidenceState::Ready));
        Ok(VerificationReceipt {
            probe_ids: plan.verification_probe_ids.clone(),
            observed_at_unix_ms: snapshot.checked_at_unix_ms,
            evidence_digest: digest(&snapshot, &plan.verification_probe_ids)?,
            ready,
        })
    }
}

fn collect(probe_ids: &[String]) -> anyhow::Result<MachineSnapshotV1> {
    let generation = crate::persistence::harness_settings::current_unix_ms().max(1);
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let scan: SetupScanResult = setup_attention::scan(generation, cancel);
    anyhow::ensure!(!scan.cancelled, "fresh Windows setup scan was cancelled");
    let snapshot = scan
        .snapshot
        .ok_or_else(|| anyhow::anyhow!("fresh Windows setup scan lacks evidence"))?;
    anyhow::ensure!(
        snapshot.host == HostKind::Windows,
        "current host is not Windows"
    );
    snapshot.validate()?;
    for probe in probe_ids {
        anyhow::ensure!(
            find_evidence(&snapshot, probe).is_some(),
            "fresh Windows setup scan lacks a required probe"
        );
    }
    Ok(snapshot)
}

fn evidence_state(
    snapshot: &MachineSnapshotV1,
    capability: PrerequisiteCapability,
) -> Option<EvidenceState> {
    snapshot.probe_evidence.iter().find_map(|evidence| {
        (evidence.prerequisite.domain == PrerequisiteDomain::Host
            && evidence.prerequisite.platform == HostKind::Windows
            && evidence.prerequisite.capability == capability)
            .then_some(evidence.state)
    })
}

fn find_evidence<'a>(
    snapshot: &'a MachineSnapshotV1,
    probe_id: &str,
) -> Option<&'a crate::app::setup_attention::catalog::ProbeEvidenceV1> {
    let capability = match probe_id {
        "host:windows:edition" => PrerequisiteCapability::HostEdition,
        "host:windows:architecture" => PrerequisiteCapability::HostArchitecture,
        "host:windows:virtualization" => PrerequisiteCapability::Virtualization,
        "host:windows:wsl_policy" => PrerequisiteCapability::WindowsWslPolicy,
        "host:windows:wsl_feature" => PrerequisiteCapability::WindowsWslFeature,
        "host:windows:vm_platform_feature" => PrerequisiteCapability::WindowsVmPlatformFeature,
        "host:windows:reboot" => PrerequisiteCapability::WindowsReboot,
        _ => return None,
    };
    snapshot.probe_evidence.iter().find(|evidence| {
        evidence.prerequisite.domain == PrerequisiteDomain::Host
            && evidence.prerequisite.platform == HostKind::Windows
            && evidence.prerequisite.capability == capability
    })
}

fn digest(snapshot: &MachineSnapshotV1, probe_ids: &[String]) -> anyhow::Result<String> {
    let selected = probe_ids
        .iter()
        .map(|id| {
            find_evidence(snapshot, id)
                .ok_or_else(|| anyhow::anyhow!("fresh Windows setup evidence is incomplete"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&selected)?)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Production boundary: enforces the broker's exact probe contract without
    // running Windows optional-feature APIs. Same-process unit tests cannot
    // prove native Windows behavior; no Docker, SSH, or host mutation is used,
    // and setup cost is negligible. Native OS validation is separately deferred.
    #[test]
    fn windows_probe_contract_is_closed() {
        assert_eq!(PRECONDITIONS.len(), 7);
        assert_eq!(VERIFICATIONS.len(), 3);
        assert!(find_evidence(&empty_snapshot(), "host:windows:unapproved_command").is_none());
    }

    fn empty_snapshot() -> MachineSnapshotV1 {
        use crate::app::setup_attention::catalog::*;
        MachineSnapshotV1 {
            schema_version: MachineSnapshotV1::SCHEMA_VERSION,
            scan_generation: 1,
            scan_started_at_unix_ms: 1,
            host: HostKind::Windows,
            setup_provider: None,
            selected_provider: None,
            checked_at_unix_ms: 1,
            capability_matrix: HostCapabilityMatrixV1 {
                native_controller: SupportClaim::Planned,
                observed_host_architecture: ImageArchitectureV1::Amd64,
                supported_runtime_architectures: vec![],
                linux_distribution: EvidenceState::NotApplicable,
                linux_distribution_id: None,
                linux_distribution_version: None,
                linux_distribution_support: None,
                linux_uid_mappings: EvidenceState::NotApplicable,
                linux_service_manager: EvidenceState::NotApplicable,
                linux_package_manager: EvidenceState::NotApplicable,
                linux_rootless_mechanism: None,
                windows_wsl2: EvidenceState::Unknown,
                virtualization: EvidenceState::Unknown,
                docker_desktop_vm_trust: EvidenceState::Unknown,
            },
            probe_evidence: vec![],
            prerequisite_graph: vec![],
            findings: vec![],
            checked_domains: vec![],
        }
    }
}
