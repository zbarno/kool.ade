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
    pub(super) base_image_path: PathBuf,
    pub(super) base_artifact_size_bytes: u64,
    pub(super) image_digest: String,
    pub(super) manifest_digest: String,
    pub(super) signer_key_id: String,
    pub(super) catalog_generation: u64,
    pub(super) virtual_size_bytes: u64,
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
    #[cfg(test)]
    pub(crate) fn for_cleanup_test(path: PathBuf, directory: PathBuf) -> Self {
        Self {
            session_id: "cleanup-test-session".into(),
            path,
            directory,
            base_image_path: PathBuf::new(),
            base_artifact_size_bytes: 0,
            image_digest: String::new(),
            manifest_digest: String::new(),
            signer_key_id: String::new(),
            catalog_generation: 0,
            virtual_size_bytes: 0,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn image_digest(&self) -> &str {
        &self.image_digest
    }

    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    pub fn signer_key_id(&self) -> &str {
        &self.signer_key_id
    }

    pub fn catalog_generation(&self) -> u64 {
        self.catalog_generation
    }

    pub fn virtual_size_bytes(&self) -> u64 {
        self.virtual_size_bytes
    }

    /// Rechecks both the immutable base digest and qcow2 backing chain at boot.
    pub fn verify_before_boot(&self) -> Result<(), GuestImageOverlayErrorV1> {
        super::verify_overlay_before_boot(self)
    }

    /// The VM backend must stop the VM before releasing its disposable overlay.
    pub fn release_after_vm_stopped(&self) -> Result<(), GuestImageOverlayErrorV1> {
        super::validate_private_directory(&self.directory)?;
        let metadata = match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(GuestImageOverlayErrorV1::Io),
        };
        if !metadata.file_type().is_file() || self.path.parent() != Some(self.directory.as_path()) {
            return Err(GuestImageOverlayErrorV1::UnsafePath);
        }
        std::fs::remove_file(&self.path).map_err(|_| GuestImageOverlayErrorV1::Io)
    }
}
