//! Signed Computer Use guest-image catalog, verified cache, and disposable overlays.

mod cache;
mod floor;
mod manifest;
mod overlay;
mod process;

#[cfg(test)]
#[path = "guest_image/tests.rs"]
mod tests;

pub use cache::{
    CachedGuestImageV1, ComputerImageDownloadApprovalV1, GuestImageCacheV1, ImageCacheErrorV1,
};
pub use floor::GuestImageCatalogGenerationFloorV1;
pub use manifest::{
    ComputerGuestArchitectureV1, GuestImageEntryV1, GuestImageManifestTrustV1,
    GuestImageManifestV1, GuestImageStatusV1, VerifiedGuestImageV1, verify_guest_image_manifest,
};
pub use overlay::{
    GuestImageOverlayErrorV1, GuestImageOverlayRequestV1, GuestImageOverlayV1,
    create_guest_image_overlay, verify_guest_image_overlay_before_boot,
};
pub use process::{QemuImageToolV1, QemuProcessError, probe_host_qemu_image_tool};
