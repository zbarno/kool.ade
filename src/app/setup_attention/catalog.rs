//! Typed bootstrap evidence and pure prerequisite classification.
//!
//! Bounded host, provider, Docker, and Git probes feed this model. Raw command
//! output, credentials, and host-specific paths are not part of its snapshot.

pub const SCHEMA_VERSION: u8 = MachineSnapshotV1::SCHEMA_VERSION;

mod assess;
mod dependencies;
mod evidence;
mod images;
mod model;
#[cfg(test)]
mod tests;

pub use model::{
    EvidenceCode, EvidenceOrigin, EvidenceState, GitProbeV1, HostCapabilityMatrixV1, HostKind,
    ImageArchitectureV1, ImageProfileV1, LinuxDistributionV1, LinuxDistributionVersionV1,
    LinuxKernelVersionV1, LinuxPackageManagerV1, LinuxRootlessMechanismV1, LinuxRootlessRecipeV1,
    MachineSnapshotV1, PrerequisiteCapability, PrerequisiteDomain, PrerequisiteId,
    PrerequisiteNodeV1, PrerequisiteSubject, ProbeEvidenceV1, ProbeObservationV1, ProviderIdV1,
    RemediationEligibility, SupportClaim,
};
pub type MachineSnapshot = MachineSnapshotV1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingStatus {
    NeedsAttention,
    BlockedByPrerequisite,
    Unverified,
    Unsupported,
}

/// A specific fault, not a generic "Docker needs attention" board card.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    pub title: String,
    pub explanation: String,
    pub next_step: String,
    pub status: FindingStatus,
    pub prerequisites: Vec<String>,
}

impl Finding {
    pub(super) fn action(id: impl Into<String>, title: &str, why: &str, next: &str) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            explanation: why.into(),
            next_step: next.into(),
            status: FindingStatus::NeedsAttention,
            prerequisites: Vec::new(),
        }
    }

    pub(super) fn unverified(id: impl Into<String>, title: &str, why: &str, next: &str) -> Self {
        Self {
            status: FindingStatus::Unverified,
            ..Self::action(id, title, why, next)
        }
    }
}

#[cfg(test)]
pub use assess::assess;
#[cfg(test)]
pub use assess::assess_for_scan;
#[cfg(test)]
pub use assess::assess_for_scan_with_git;
pub(crate) use assess::probe_git;
pub(crate) use assess::{ImageScanState, ScanInputs, assess_for_scan_with_inputs};
