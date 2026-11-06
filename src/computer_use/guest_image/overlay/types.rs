use std::path::{Path, PathBuf};

use ed25519_dalek::VerifyingKey;

use crate::computer_use::ComputerSessionV1;

use super::super::{
    ComputerGuestArchitectureV1, GuestImageCatalogGenerationFloorV1, GuestImageManifestTrustV1,
    ImageCacheErrorV1, process::QemuImageToolV1,
};

#[derive(Debug, Eq, PartialEq)]
pub enum GuestImageOverlayErrorV1 {
    InvalidSessionId,
    UnsafePath,
    BaseImageChanged,
    QemuImageUnavailable,
    QemuImageFailed,
    InvalidOverlay,
    ManifestRejected(ImageCacheErrorV1),
    CatalogRollback,
    CatalogFloorUnavailable,
    Io,
}

impl std::fmt::Display for GuestImageOverlayErrorV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "computer-use overlay error: {self:?}")
    }
}

impl std::error::Error for GuestImageOverlayErrorV1 {}

#[derive(Debug)]
pub struct GuestImageOverlayV1 {
    pub(super) session_id: String,
    pub(super) path: PathBuf,
    pub(super) directory: PathBuf,
}

/// Current trust and launch inputs that must be revalidated at the overlay boundary.
pub struct GuestImageOverlayRequestV1<'a> {
    pub manifest_bytes: &'a [u8],
    pub trust: &'a GuestImageManifestTrustV1,
    pub session: &'a ComputerSessionV1,
    pub expected_architecture: ComputerGuestArchitectureV1,
    pub controller_key_id: &'a str,
    pub controller_key: &'a VerifyingKey,
    pub generation_floor: &'a GuestImageCatalogGenerationFloorV1,
    pub overlay_root: &'a Path,
    pub qemu_tool: &'a QemuImageToolV1,
}

impl GuestImageOverlayV1 {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The VM backend must stop the VM before releasing its disposable overlay.
    pub fn release_after_vm_stopped(self) -> Result<(), GuestImageOverlayErrorV1> {
        super::validate_private_directory(&self.directory)?;
        let metadata =
            std::fs::symlink_metadata(&self.path).map_err(|_| GuestImageOverlayErrorV1::Io)?;
        if !metadata.file_type().is_file() || self.path.parent() != Some(self.directory.as_path()) {
            return Err(GuestImageOverlayErrorV1::UnsafePath);
        }
        std::fs::remove_file(&self.path).map_err(|_| GuestImageOverlayErrorV1::Io)
    }
}
