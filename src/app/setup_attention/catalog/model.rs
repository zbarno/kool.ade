mod evidence;
mod identifiers;
mod install_channels;
mod recipe;
mod snapshot;

pub use evidence::{
    EvidenceCode, EvidenceOrigin, EvidenceState, GitProbeV1, PrerequisiteNodeV1, ProbeEvidenceV1,
    ProbeObservationV1, RemediationEligibility,
};
pub use identifiers::{
    HostKind, ImageArchitectureV1, ImageProfileV1, LinuxDistributionV1, LinuxDistributionVersionV1,
    LinuxKernelVersionV1, LinuxPackageManagerV1, LinuxRootlessMechanismV1, LinuxRootlessRecipeV1,
    PrerequisiteCapability, PrerequisiteDomain, PrerequisiteId, PrerequisiteSubject, ProviderIdV1,
    WindowsEditionV1, WindowsKernelVersionV1, WindowsProductV1,
};
pub use snapshot::{HostCapabilityMatrixV1, MachineSnapshotV1, SupportClaim};
