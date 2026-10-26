use serde::{Deserialize, Serialize};

use super::{
    evidence::{EvidenceState, PrerequisiteNodeV1, ProbeEvidenceV1, ProbeObservationV1},
    identifiers::{
        HostKind, ImageArchitectureV1, PrerequisiteCapability, PrerequisiteDomain, ProviderIdV1,
    },
};
use crate::app::setup_attention::catalog::Finding;

mod claims;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportClaim {
    Experimental,
    Planned,
    Unverified,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HostCapabilityMatrixV1 {
    pub native_controller: SupportClaim,
    pub observed_host_architecture: ImageArchitectureV1,
    pub supported_runtime_architectures: Vec<ImageArchitectureV1>,
    pub linux_distribution: EvidenceState,
    pub linux_distribution_id: Option<super::identifiers::LinuxDistributionV1>,
    pub linux_distribution_version: Option<super::identifiers::LinuxDistributionVersionV1>,
    pub linux_distribution_support: Option<SupportClaim>,
    pub linux_uid_mappings: EvidenceState,
    pub linux_service_manager: EvidenceState,
    pub linux_package_manager: EvidenceState,
    pub linux_rootless_mechanism: Option<super::identifiers::LinuxRootlessMechanismV1>,
    pub windows_wsl2: EvidenceState,
    pub virtualization: EvidenceState,
    pub docker_desktop_vm_trust: EvidenceState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MachineSnapshotV1 {
    pub schema_version: u8,
    pub scan_generation: u64,
    /// Captured before probes start; durable reconciliation uses this to fence
    /// an older slow scan that finishes after a newer scan.
    pub scan_started_at_unix_ms: u64,
    pub host: HostKind,
    /// Provider explicitly chosen to guide machine setup. This is separate
    /// from the selected default used for project routing.
    pub setup_provider: Option<ProviderIdV1>,
    pub selected_provider: Option<ProviderIdV1>,
    pub checked_at_unix_ms: u64,
    pub capability_matrix: HostCapabilityMatrixV1,
    pub probe_evidence: Vec<ProbeEvidenceV1>,
    pub prerequisite_graph: Vec<PrerequisiteNodeV1>,
    pub findings: Vec<Finding>,
    /// Only these domains may have stale tasks marked resolved.
    pub checked_domains: Vec<String>,
}

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
            let observation_matches = matches!(
                (
                    item.prerequisite.domain,
                    item.prerequisite.capability,
                    &item.observation,
                ),
                (
                    PrerequisiteDomain::Host,
                    PrerequisiteCapability::HostArchitecture,
                    Some(ProbeObservationV1::HostArchitecture { .. }),
                ) | (
                    PrerequisiteDomain::Host,
                    PrerequisiteCapability::LinuxDistribution,
                    Some(ProbeObservationV1::LinuxDistribution { .. }),
                ) | (
                    PrerequisiteDomain::Docker,
                    PrerequisiteCapability::DockerEndpoint,
                    Some(ProbeObservationV1::DockerEngineIdentity { .. }),
                ) | (_, _, None)
            );
            anyhow::ensure!(
                observation_matches,
                "probe observation does not match its prerequisite"
            );
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
        claims::validate(self)?;
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
