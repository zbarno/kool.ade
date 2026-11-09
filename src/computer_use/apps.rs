//! Signed, app-ID-based guest application catalog and scoped operations.

mod catalog;
#[cfg(target_os = "linux")]
pub mod guest_helper;
#[cfg(target_os = "linux")]
pub use guest_helper::GuestAppHelperConfigV1;
mod operations;
mod staged_task;

pub use catalog::{
    GuestAppArchitectureV1, GuestAppCapabilityV1, GuestAppInstallRecipeV1, GuestAppManifestV1,
    GuestAppPackageSourceV1, GuestAppProvenanceV1, GuestAppRemovalV1, GuestAppRollbackV1,
    GuestAppStatusV1, GuestApplicationCatalogTrustV1, GuestApplicationCatalogV1,
    VerifiedGuestApplicationCatalogV1,
};
pub use operations::{
    GuestAppConsentGrantV1, GuestAppOperationKindV1, GuestAppOperationRequestV1,
    GuestAppProvisionerV1,
};
pub use staged_task::{
    StagedTaskApplicationBuildV1, StagedTaskApplicationV1, StagedTaskArtifactContentV1,
    StagedTaskArtifactV1, StagedTaskBuildFileV1,
};
