use serde::{Deserialize, Serialize};

use super::{
    evidence::{EvidenceState, PrerequisiteNodeV1, ProbeEvidenceV1},
    identifiers::{HostKind, ImageArchitectureV1, ProviderIdV1},
};
use crate::app::setup_attention::catalog::Finding;

mod claims;
#[cfg(test)]
mod validation_tests;

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

mod validation;
