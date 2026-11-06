//! Signed Computer Use guest-image catalog, verified cache, and disposable overlays.

mod cache;
mod manifest;
mod overlay;

#[cfg(test)]
#[path = "guest_image/tests.rs"]
mod tests;

pub use cache::{
    CachedGuestImageV1, ComputerImageDownloadApprovalV1, GuestImageCacheV1, ImageCacheErrorV1,
};
pub use manifest::{
    ComputerGuestArchitectureV1, GuestImageEntryV1, GuestImageManifestTrustV1,
    GuestImageManifestV1, GuestImageStatusV1, VerifiedGuestImageV1, verify_guest_image_manifest,
};
pub use overlay::{GuestImageOverlayErrorV1, GuestImageOverlayV1, create_guest_image_overlay};
