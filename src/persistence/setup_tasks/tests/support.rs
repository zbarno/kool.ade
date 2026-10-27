use crate::app::setup_attention::catalog::{
    EvidenceCode, EvidenceOrigin, EvidenceState, Finding, FindingStatus, HostCapabilityMatrixV1,
    HostKind, ImageArchitectureV1, ImageProfileV1, MachineSnapshot, PrerequisiteCapability,
    PrerequisiteDomain, PrerequisiteId, PrerequisiteSubject, ProbeEvidenceV1, ProbeObservationV1,
    SCHEMA_VERSION, SupportClaim,
};

use super::super::{SetupTaskStateV1, SetupTaskStoreV1};

pub fn snapshot(
    findings: &[(&str, FindingStatus, &[&str])],
    domains: &[&str],
    at: u64,
) -> MachineSnapshot {
    let observed_host_architecture = if domains.contains(&"images:general:arm64") {
        ImageArchitectureV1::Arm64
    } else {
        ImageArchitectureV1::Amd64
    };
    MachineSnapshot {
        schema_version: SCHEMA_VERSION,
        scan_generation: 1,
        scan_started_at_unix_ms: at,
        host: HostKind::Linux,
        setup_provider: None,
        selected_provider: None,
        checked_at_unix_ms: at,
        capability_matrix: HostCapabilityMatrixV1 {
            native_controller: SupportClaim::Unverified,
            observed_host_architecture,
            supported_runtime_architectures: Vec::new(),
            linux_distribution: EvidenceState::Unknown,
            linux_distribution_id: None,
            linux_distribution_version: None,
            linux_distribution_support: None,
            linux_uid_mappings: EvidenceState::Unknown,
            linux_service_manager: EvidenceState::Unknown,
            linux_package_manager: EvidenceState::Unknown,
            linux_rootless_mechanism: None,
            windows_wsl2: EvidenceState::Unknown,
            virtualization: EvidenceState::Unknown,
            docker_desktop_vm_trust: EvidenceState::Unknown,
        },
        probe_evidence: Vec::new(),
        prerequisite_graph: Vec::new(),
        findings: findings
            .iter()
            .map(|(id, status, prerequisites)| Finding {
                id: (*id).into(),
                title: (*id).into(),
                explanation: "synthetic readiness finding".into(),
                next_step: "retry the readiness scan".into(),
                status: *status,
                prerequisites: prerequisites.iter().map(|value| (*value).into()).collect(),
            })
            .collect(),
        checked_domains: domains
            .iter()
            .map(|domain| match *domain {
                "docker:rootless" | "docker:capabilities" => "docker",
                "cli:codex:auth" => "cli:codex",
                other => other,
            })
            .map(str::to_owned)
            .collect(),
    }
}

pub fn rootless_smoke_fact(scan_generation: u64, completed_at_unix_ms: u64) -> ProbeEvidenceV1 {
    let image = crate::execution::docker::images::ImageLock::bundled()
        .unwrap()
        .select(
            crate::execution::docker::images::RuntimeProfile::General,
            "amd64",
        )
        .unwrap();
    ProbeEvidenceV1 {
        prerequisite: PrerequisiteId {
            domain: PrerequisiteDomain::Docker,
            platform: HostKind::Linux,
            capability: PrerequisiteCapability::DockerRootlessSmoke,
            subject: PrerequisiteSubject::Image {
                profile: ImageProfileV1::General,
                architecture: ImageArchitectureV1::Amd64,
            },
        },
        state: EvidenceState::Ready,
        code: EvidenceCode::ApprovedImageSmokeSucceeded,
        origin: EvidenceOrigin::ApprovedImageSmokeTest,
        scan_generation,
        source_generation: None,
        observed_at_unix_ms: completed_at_unix_ms,
        observation: Some(ProbeObservationV1::DockerRootlessSmoke {
            endpoint_fingerprint: "a".repeat(64),
            daemon_fingerprint: "b".repeat(64),
            daemon_version: "29.0.0".into(),
            image_reference: image.reference,
            image_digest: image.digest,
            image_platform: image.platform,
            completed_at_unix_ms,
        }),
    }
}

pub fn scan(store: &mut SetupTaskStoreV1, snapshot: &MachineSnapshot) -> anyhow::Result<bool> {
    store.latest_scan_generation += 1;
    let mut current = snapshot.clone();
    current.scan_generation = store.latest_scan_generation;
    store.reconcile(&current, store.latest_scan_generation)
}

pub fn ready_store(id: &str) -> SetupTaskStoreV1 {
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(
            &[(id, FindingStatus::NeedsAttention, &[])],
            &["docker:rootless"],
            1,
        ),
    )
    .unwrap();
    assert_eq!(store.tasks[id].state, SetupTaskStateV1::ReadyToFix);
    store
}
