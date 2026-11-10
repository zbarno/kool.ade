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
    #[cfg(target_os = "linux")]
    pub(crate) persistent_disk_key:
        Option<crate::computer_use::personal_storage::PersonalVmDiskKeyV1>,
    #[cfg(target_os = "linux")]
    pub(crate) _profile_lock: Option<std::fs::File>,
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
    #[cfg(target_os = "linux")]
    pub(crate) fn from_personal_profile(
        session_id: String,
        profile: &crate::computer_use::PersonalVmProfileV1,
        image: &super::super::CachedGuestImageV1,
        overlay_root: &Path,
        key: crate::computer_use::personal_storage::PersonalVmDiskKeyV1,
        profile_lock: std::fs::File,
    ) -> Result<Self, GuestImageOverlayErrorV1> {
        if !super::is_canonical_uuid(&session_id) {
            return Err(GuestImageOverlayErrorV1::InvalidSessionId);
        }
        if profile.image_digest != image.entry().image_digest
            || profile.image_manifest_digest != image.manifest_digest()
            || profile.image_signer_key_id != image.manifest_signer_key_id()
            || profile.catalog_generation != image.catalog_generation()
            || profile.virtual_size_bytes != image.entry().image_size_bytes
        {
            return Err(GuestImageOverlayErrorV1::BaseImageChanged);
        }
        super::validate_private_directory(overlay_root)?;
        Ok(Self {
            session_id,
            path: overlay_root.join(&profile.disk_file_name),
            directory: overlay_root.to_path_buf(),
            base_image_path: image.path().to_path_buf(),
            base_artifact_size_bytes: image.entry().artifact_size_bytes,
            image_digest: image.entry().image_digest.clone(),
            manifest_digest: image.manifest_digest().to_owned(),
            signer_key_id: image.manifest_signer_key_id().to_owned(),
            catalog_generation: image.catalog_generation(),
            virtual_size_bytes: profile.virtual_size_bytes,
            persistent_disk_key: Some(key),
            _profile_lock: Some(profile_lock),
        })
    }

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
            #[cfg(target_os = "linux")]
            persistent_disk_key: None,
            #[cfg(target_os = "linux")]
            _profile_lock: None,
        }
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn for_persistent_cleanup_test(
        path: PathBuf,
        directory: PathBuf,
        key: crate::computer_use::personal_storage::PersonalVmDiskKeyV1,
        profile_lock: std::fs::File,
    ) -> Self {
        let mut overlay = Self::for_cleanup_test(path, directory);
        overlay.persistent_disk_key = Some(key);
        overlay._profile_lock = Some(profile_lock);
        overlay
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

    #[cfg(target_os = "linux")]
    pub(crate) fn is_persistent_personal(&self) -> bool {
        self.persistent_disk_key.is_some() && self._profile_lock.is_some()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn persistent_key_fd(
        &self,
    ) -> Result<Option<std::fs::File>, super::super::GuestImageOverlayErrorV1> {
        self.persistent_disk_key
            .as_ref()
            .map(crate::computer_use::personal_storage::PersonalVmDiskKeyV1::as_qemu_secret_fd)
            .transpose()
            .map_err(|_| super::super::GuestImageOverlayErrorV1::Io)
    }

    /// Rechecks both the immutable base digest and qcow2 backing chain at boot.
    pub fn verify_before_boot(&self) -> Result<(), GuestImageOverlayErrorV1> {
        super::verify_overlay_before_boot(self)
    }

    /// The VM backend must stop the VM before releasing its disposable overlay.
    pub fn release_after_vm_stopped(&self) -> Result<(), GuestImageOverlayErrorV1> {
        #[cfg(target_os = "linux")]
        if self.persistent_disk_key.is_some() {
            return Ok(());
        }
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
