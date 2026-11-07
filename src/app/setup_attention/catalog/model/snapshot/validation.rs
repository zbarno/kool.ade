use super::super::{
    evidence::{EvidenceState, ProbeEvidenceV1, ProbeObservationV1},
    identifiers::{
        HostKind, ImageArchitectureV1, ImageProfileV1, PrerequisiteCapability, PrerequisiteDomain,
        PrerequisiteSubject,
    },
};
use super::MachineSnapshotV1;

mod helpers;
use helpers::valid_sha256;
pub(super) use helpers::{
    requires_macos_release, rootless_smoke_subject_matches, valid_macos_version,
};

impl MachineSnapshotV1 {
    pub const SCHEMA_VERSION: u8 = 2;

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == Self::SCHEMA_VERSION,
            "unsupported machine readiness snapshot version"
        );
        anyhow::ensure!(
            self.scan_started_at_unix_ms <= self.checked_at_unix_ms,
            "machine readiness scan finished before it started"
        );
        anyhow::ensure!(
            self.probe_evidence.len() <= 256,
            "too much machine readiness evidence"
        );
        anyhow::ensure!(
            self.prerequisite_graph.len() <= 256,
            "machine readiness graph is oversized"
        );
        anyhow::ensure!(self.findings.len() <= 128, "too many setup findings");
        anyhow::ensure!(
            self.checked_domains.len() <= 128,
            "too many checked domains"
        );
        let evidence_ids = self
            .probe_evidence
            .iter()
            .map(|item| &item.prerequisite)
            .collect::<std::collections::BTreeSet<_>>();
        anyhow::ensure!(
            evidence_ids.len() == self.probe_evidence.len(),
            "duplicate machine readiness evidence"
        );
        anyhow::ensure!(
            self.probe_evidence
                .iter()
                .all(|item| item.prerequisite.platform == self.host),
            "probe evidence platform conflicts with its snapshot"
        );
        for item in &self.probe_evidence {
            anyhow::ensure!(
                item.observed_at_unix_ms >= self.scan_started_at_unix_ms
                    && item.observed_at_unix_ms <= self.checked_at_unix_ms,
                "probe evidence timestamp {} falls outside scan interval {}..{} for {:?}",
                item.observed_at_unix_ms,
                self.scan_started_at_unix_ms,
                self.checked_at_unix_ms,
                item.prerequisite
            );
        }
        for item in &self.probe_evidence {
            anyhow::ensure!(
                helpers::observation_matches(item),
                "probe observation does not match its prerequisite"
            );
            if item.prerequisite.capability == PrerequisiteCapability::LinuxKvmDevice {
                anyhow::ensure!(
                    helpers::linux_kvm_evidence_is_consistent(item),
                    "KVM device evidence conflicts with its probe result"
                );
            }
            if item.prerequisite.capability == PrerequisiteCapability::LinuxVirtualization {
                anyhow::ensure!(
                    helpers::linux_virtualization_evidence_is_consistent(item),
                    "Linux virtualization evidence conflicts with its probe result"
                );
            }
            if item.prerequisite.capability == PrerequisiteCapability::LinuxQemu {
                anyhow::ensure!(
                    helpers::linux_qemu_evidence_is_consistent(item),
                    "QEMU evidence conflicts with its probe result"
                );
            }
            if item.prerequisite.capability == PrerequisiteCapability::LinuxVmResources {
                anyhow::ensure!(
                    helpers::linux_vm_resources_evidence_is_consistent(item),
                    "Linux VM resource evidence is inconsistent"
                );
            }
            let is_computer_component = matches!(
                item.prerequisite.capability,
                PrerequisiteCapability::ComputerGuestImage
                    | PrerequisiteCapability::ComputerOverlay
                    | PrerequisiteCapability::ComputerNetworkGateway
                    | PrerequisiteCapability::ComputerGraphicsTransport
                    | PrerequisiteCapability::ComputerEncryptionKeyService
                    | PrerequisiteCapability::ComputerGuestProtocol
            );
            if is_computer_component
                || item.code
                    == super::super::evidence::EvidenceCode::ComputerUseReadinessProbeNotIntegrated
                || item.origin == super::super::evidence::EvidenceOrigin::ApplicationInventory
            {
                anyhow::ensure!(
                    is_computer_component
                        && helpers::computer_use_capability_evidence_is_consistent(
                            item,
                            self.scan_generation
                        ),
                    "Computer Use component evidence conflicts with the current build inventory"
                );
            }
            if requires_macos_release(item) {
                anyhow::ensure!(
                    item.code == super::super::evidence::EvidenceCode::MacOsReleaseDetected,
                    "ready host edition evidence lacks a verified macOS release"
                );
            }
            if item.code == super::super::evidence::EvidenceCode::MacOsReleaseDetected {
                let Some(ProbeObservationV1::MacOsVersion { version }) = &item.observation else {
                    anyhow::bail!("macOS release evidence lacks its typed version observation");
                };
                anyhow::ensure!(
                    item.prerequisite.platform == HostKind::MacOs
                        && item.prerequisite.domain == PrerequisiteDomain::Host
                        && item.prerequisite.capability == PrerequisiteCapability::HostEdition
                        && item.prerequisite.subject == PrerequisiteSubject::None
                        && item.state == EvidenceState::Ready
                        && item.code == super::super::evidence::EvidenceCode::MacOsReleaseDetected
                        && item.origin == super::super::evidence::EvidenceOrigin::HostRuntime
                        && item.source_generation.is_none()
                        && valid_macos_version(version),
                    "macOS version observation is not valid host release evidence"
                );
            }
            if let Some(ProbeObservationV1::MacOsVersion { .. }) = &item.observation {
                anyhow::ensure!(
                    item.code == super::super::evidence::EvidenceCode::MacOsReleaseDetected,
                    "macOS version observation lacks matching release evidence"
                );
            }
            anyhow::ensure!(
                super::claims::windows_observation_is_consistent(item),
                "Windows probe observation conflicts with its readiness state"
            );
            if item.prerequisite.capability == PrerequisiteCapability::DockerRootlessSmoke {
                let Some(ProbeObservationV1::DockerRootlessSmoke {
                    endpoint_fingerprint,
                    daemon_fingerprint,
                    daemon_version,
                    image_reference,
                    image_digest,
                    image_platform,
                    completed_at_unix_ms,
                }) = &item.observation
                else {
                    anyhow::bail!("rootless smoke evidence lacks its typed receipt");
                };
                anyhow::ensure!(
                    item.prerequisite.domain == PrerequisiteDomain::Docker
                        && item.state == EvidenceState::Ready
                        && item.code
                            == super::super::evidence::EvidenceCode::ApprovedImageSmokeSucceeded
                        && item.origin
                            == super::super::evidence::EvidenceOrigin::ApprovedImageSmokeTest
                        && item.source_generation.is_none()
                        && valid_sha256(endpoint_fingerprint)
                        && valid_sha256(daemon_fingerprint)
                        && !daemon_version.is_empty()
                        && daemon_version.len() <= 128
                        && !daemon_version.bytes().any(|byte| byte.is_ascii_control())
                        && image_reference.len() <= 512
                        && !image_reference.is_empty()
                        && !image_reference.bytes().any(|byte| byte.is_ascii_control())
                        && image_digest
                            .strip_prefix("sha256:")
                            .is_some_and(valid_sha256)
                        && rootless_smoke_subject_matches(
                            &item.prerequisite.subject,
                            image_platform,
                        )
                        && *completed_at_unix_ms > 0
                        && *completed_at_unix_ms <= self.checked_at_unix_ms,
                    "rootless smoke evidence contains an invalid receipt"
                );
            }
        }
        let computer_components = [
            PrerequisiteCapability::ComputerGuestImage,
            PrerequisiteCapability::ComputerOverlay,
            PrerequisiteCapability::ComputerNetworkGateway,
            PrerequisiteCapability::ComputerGraphicsTransport,
            PrerequisiteCapability::ComputerEncryptionKeyService,
            PrerequisiteCapability::ComputerGuestProtocol,
        ];
        if self.host == HostKind::Linux
            && (self
                .probe_evidence
                .iter()
                .any(|item| computer_components.contains(&item.prerequisite.capability))
                || self
                    .findings
                    .iter()
                    .any(|finding| finding.id.starts_with("setup:computer:")))
        {
            for capability in computer_components {
                let item = self.probe_evidence.iter().find(|item| {
                    item.prerequisite.capability == capability
                        && item.prerequisite.domain == PrerequisiteDomain::Host
                        && item.prerequisite.platform == HostKind::Linux
                        && item.prerequisite.subject == PrerequisiteSubject::None
                });
                anyhow::ensure!(
                    item.is_some_and(|item| {
                        helpers::computer_use_capability_evidence_is_consistent(
                            item,
                            self.scan_generation,
                        )
                    }),
                    "Computer Use component inventory is incomplete or inconsistent"
                );
            }
        }
        let nodes = self
            .prerequisite_graph
            .iter()
            .map(|node| &node.id)
            .collect::<std::collections::BTreeSet<_>>();
        anyhow::ensure!(
            nodes.len() == self.prerequisite_graph.len(),
            "duplicate prerequisite graph node"
        );
        anyhow::ensure!(
            nodes == evidence_ids,
            "prerequisite graph and evidence do not match"
        );
        for node in &self.prerequisite_graph {
            let evidence = self
                .probe_evidence
                .iter()
                .find(|item| item.prerequisite == node.id)
                .ok_or_else(|| anyhow::anyhow!("prerequisite graph node lacks probe evidence"))?;
            anyhow::ensure!(
                node.state == evidence.state,
                "prerequisite graph state conflicts with probe evidence"
            );
            anyhow::ensure!(
                node.depends_on
                    .iter()
                    .all(|dependency| nodes.contains(dependency) && dependency != &node.id),
                "invalid prerequisite dependency"
            );
        }
        super::claims::validate(self)?;
        let mut resolved = std::collections::BTreeSet::new();
        while resolved.len() < nodes.len() {
            let before = resolved.len();
            for node in &self.prerequisite_graph {
                if node
                    .depends_on
                    .iter()
                    .all(|dependency| resolved.contains(dependency))
                {
                    resolved.insert(&node.id);
                }
            }
            anyhow::ensure!(
                resolved.len() > before,
                "prerequisite graph contains a cycle"
            );
        }
        Ok(())
    }
}
