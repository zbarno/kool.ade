mod download;
mod storage;
#[cfg(test)]
mod tests;
mod transport;

use std::{
    fs,
    path::{Path, PathBuf},
};

use self::storage::{
    path_is_occupied, prepare_private_directory, sync_directory, validate_private_directory,
    verify_file,
};

use super::floor::GuestImageCatalogGenerationFloorV1;
use super::manifest::{
    ComputerGuestArchitectureV1, GuestImageEntryV1, GuestImageManifestTrustV1,
    VerifiedGuestImageV1, verify_guest_image_catalog,
};

#[derive(Debug, Eq, PartialEq)]
pub enum ImageCacheErrorV1 {
    InvalidManifest(&'static str),
    InvalidTrustStore,
    ManifestTooLarge,
    StaleCatalog,
    CatalogExpired,
    CatalogRollback,
    CatalogFloorUnavailable,
    UntrustedManifestSigner,
    SessionManifestMismatch,
    SessionAttestationInvalid,
    ImageNotCatalogued,
    ImageRevoked,
    UnsupportedSessionClass,
    ApprovalDenied,
    DownloadFailed,
    DownloadTooLarge,
    DownloadLengthMismatch,
    DigestMismatch,
    CachePathUnsafe,
    CacheCorrupt,
    Io,
}

impl std::fmt::Display for ImageCacheErrorV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "computer-use guest image error: {self:?}")
    }
}

impl std::error::Error for ImageCacheErrorV1 {}

/// App broker adapter that confirms the operator approved this exact image.
pub trait ComputerImageDownloadApprovalV1 {
    fn authorize_download(
        &self,
        manifest_digest: &str,
        image_digest: &str,
    ) -> Result<(), ImageCacheErrorV1>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CachedGuestImageV1 {
    entry: GuestImageEntryV1,
    manifest_digest: String,
    manifest_signer_key_id: String,
    catalog_generation: u64,
    path: PathBuf,
}

impl CachedGuestImageV1 {
    pub fn entry(&self) -> &GuestImageEntryV1 {
        &self.entry
    }

    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    pub fn manifest_signer_key_id(&self) -> &str {
        &self.manifest_signer_key_id
    }

    pub fn catalog_generation(&self) -> u64 {
        self.catalog_generation
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Clone, Debug)]
pub struct GuestImageCacheV1 {
    root: PathBuf,
}

impl GuestImageCacheV1 {
    pub fn new(root: PathBuf) -> Result<Self, ImageCacheErrorV1> {
        prepare_private_directory(&root)?;
        Ok(Self { root })
    }

    /// Open an already provisioned cache without creating or changing it.
    pub fn open_existing(root: PathBuf) -> Result<Self, ImageCacheErrorV1> {
        validate_private_directory(&root)?;
        Ok(Self { root })
    }

    pub fn verify_cached(
        &self,
        verified: &VerifiedGuestImageV1,
    ) -> Result<CachedGuestImageV1, ImageCacheErrorV1> {
        let path = self.image_path(&verified.entry)?;
        verify_file(&path, &verified.entry)?;
        Ok(CachedGuestImageV1 {
            entry: verified.entry.clone(),
            manifest_digest: verified.manifest_digest.clone(),
            manifest_signer_key_id: verified.manifest_signer_key_id.clone(),
            catalog_generation: verified.catalog_generation,
            path,
        })
    }

    /// Verify a previously cached approved artifact using only signed catalog
    /// metadata. This is readiness evidence only: it does not advance the
    /// durable generation floor or authorize a session. The launch path must
    /// accept the generation again immediately before creating an overlay.
    pub fn verify_cached_catalog_image(
        &self,
        catalog_bytes: &[u8],
        trust: &GuestImageManifestTrustV1,
        generation_floor: &GuestImageCatalogGenerationFloorV1,
        architecture: ComputerGuestArchitectureV1,
    ) -> Result<CachedGuestImageV1, ImageCacheErrorV1> {
        self.verify_cached_catalog_image_with_time(
            catalog_bytes,
            trust,
            generation_floor,
            architecture,
            download::current_time_unix_ms()?,
        )
    }

    #[cfg(test)]
    pub(crate) fn verify_cached_catalog_image_at(
        &self,
        catalog_bytes: &[u8],
        trust: &GuestImageManifestTrustV1,
        generation_floor: &GuestImageCatalogGenerationFloorV1,
        architecture: ComputerGuestArchitectureV1,
        now_unix_ms: u64,
    ) -> Result<CachedGuestImageV1, ImageCacheErrorV1> {
        self.verify_cached_catalog_image_with_time(
            catalog_bytes,
            trust,
            generation_floor,
            architecture,
            now_unix_ms,
        )
    }

    fn verify_cached_catalog_image_with_time(
        &self,
        catalog_bytes: &[u8],
        trust: &GuestImageManifestTrustV1,
        generation_floor: &GuestImageCatalogGenerationFloorV1,
        architecture: ComputerGuestArchitectureV1,
        now_unix_ms: u64,
    ) -> Result<CachedGuestImageV1, ImageCacheErrorV1> {
        let catalog = verify_guest_image_catalog(catalog_bytes, trust, now_unix_ms)?;
        generation_floor.require_not_rolled_back(catalog.catalog_generation())?;
        let entry = catalog
            .approved_image(architecture)
            .ok_or(ImageCacheErrorV1::ImageNotCatalogued)?;
        let path = self.image_path(entry)?;
        verify_file(&path, entry)?;
        Ok(CachedGuestImageV1 {
            entry: entry.clone(),
            manifest_digest: catalog.digest().to_owned(),
            manifest_signer_key_id: catalog.signer_key_id().to_owned(),
            catalog_generation: catalog.catalog_generation(),
            path,
        })
    }

    /// Remove only a corrupt regular image (or link) after approval to replace
    /// this exact catalog/image pair. Healthy entries and unrelated files stay.
    pub fn remove_corrupt_for_approved_redownload(
        &self,
        verified: &VerifiedGuestImageV1,
        approval: &dyn ComputerImageDownloadApprovalV1,
    ) -> Result<(), ImageCacheErrorV1> {
        approval.authorize_download(&verified.manifest_digest, &verified.entry.image_digest)?;
        let path = self.image_path(&verified.entry)?;
        if !path_is_occupied(&path)? || verify_file(&path, &verified.entry).is_ok() {
            return Ok(());
        }
        let metadata = fs::symlink_metadata(&path).map_err(|_| ImageCacheErrorV1::Io)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            return Err(ImageCacheErrorV1::CachePathUnsafe);
        }
        fs::remove_file(&path).map_err(|_| ImageCacheErrorV1::Io)?;
        sync_directory(&self.root)
    }

    fn image_path(&self, entry: &GuestImageEntryV1) -> Result<PathBuf, ImageCacheErrorV1> {
        let digest = entry
            .image_digest
            .strip_prefix("sha256:")
            .ok_or(ImageCacheErrorV1::InvalidManifest("invalid image digest"))?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(ImageCacheErrorV1::InvalidManifest("invalid image digest"));
        }
        Ok(self.root.join(format!("{digest}.qcow2")))
    }
}
