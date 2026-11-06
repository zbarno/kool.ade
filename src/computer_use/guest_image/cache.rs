mod storage;
mod transport;

use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use reqwest::StatusCode;
use uuid::Uuid;

use self::{
    storage::{
        copy_bounded, path_is_occupied, prepare_private_directory, set_read_only, sync_directory,
        verify_file,
    },
    transport::{release_client, release_tag},
};

const RELEASE_BASE: &str = "https://github.com/zbarno/kool.ade/releases/download";
use super::manifest::{GuestImageEntryV1, VerifiedGuestImageV1};

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

struct TemporaryCacheFile {
    path: PathBuf,
    installed: bool,
}

impl Drop for TemporaryCacheFile {
    fn drop(&mut self) {
        if !self.installed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl GuestImageCacheV1 {
    pub fn new(root: PathBuf) -> Result<Self, ImageCacheErrorV1> {
        prepare_private_directory(&root)?;
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

    /// Download is started only after the approval adapter authorizes this
    /// manifest and image digest. The URL is derived from the signed release
    /// tag and fixed public repository, never from task or model input.
    pub fn download_approved_release(
        &self,
        verified: &VerifiedGuestImageV1,
        approval: &dyn ComputerImageDownloadApprovalV1,
    ) -> Result<CachedGuestImageV1, ImageCacheErrorV1> {
        approval.authorize_download(&verified.manifest_digest, &verified.entry.image_digest)?;
        let path = self.image_path(&verified.entry)?;
        if path.exists() {
            return self.verify_cached(verified);
        }
        let release_tag = release_tag(&verified.catalog_version)?;
        let url = format!(
            "{RELEASE_BASE}/{release_tag}/{}",
            verified.entry.artifact_name
        );
        let client = release_client()?;
        let mut response = client
            .get(url)
            .send()
            .map_err(|_| ImageCacheErrorV1::DownloadFailed)?;
        if response.status() != StatusCode::OK {
            return Err(ImageCacheErrorV1::DownloadFailed);
        }
        if response
            .content_length()
            .is_some_and(|length| length != verified.entry.artifact_size_bytes)
        {
            return Err(ImageCacheErrorV1::DownloadLengthMismatch);
        }
        let path = self.install_reader_authorized(verified, &mut response)?;
        Ok(CachedGuestImageV1 {
            entry: verified.entry.clone(),
            manifest_digest: verified.manifest_digest.clone(),
            manifest_signer_key_id: verified.manifest_signer_key_id.clone(),
            catalog_generation: verified.catalog_generation,
            path,
        })
    }

    /// Streaming seam for deterministic tests and the bounded HTTP client.
    pub fn install_approved_reader<R: Read>(
        &self,
        verified: &VerifiedGuestImageV1,
        approval: &dyn ComputerImageDownloadApprovalV1,
        reader: &mut R,
    ) -> Result<CachedGuestImageV1, ImageCacheErrorV1> {
        approval.authorize_download(&verified.manifest_digest, &verified.entry.image_digest)?;
        let destination = self.image_path(&verified.entry)?;
        if destination.exists() {
            return self.verify_cached(verified);
        }
        self.install_reader_authorized(verified, reader)
            .map(|path| CachedGuestImageV1 {
                entry: verified.entry.clone(),
                manifest_digest: verified.manifest_digest.clone(),
                manifest_signer_key_id: verified.manifest_signer_key_id.clone(),
                catalog_generation: verified.catalog_generation,
                path,
            })
    }

    fn install_reader_authorized<R: Read>(
        &self,
        verified: &VerifiedGuestImageV1,
        reader: &mut R,
    ) -> Result<PathBuf, ImageCacheErrorV1> {
        let destination = self.image_path(&verified.entry)?;
        if path_is_occupied(&destination)? {
            return self.verify_cached(verified).map(|image| image.path);
        }
        let mut temporary = TemporaryCacheFile {
            path: self.root.join(format!(".{}.part", Uuid::new_v4())),
            installed: false,
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary.path)
            .map_err(|_| ImageCacheErrorV1::Io)?;
        copy_bounded(reader, &mut file, &verified.entry)?;
        file.flush().map_err(|_| ImageCacheErrorV1::Io)?;
        file.sync_all().map_err(|_| ImageCacheErrorV1::Io)?;
        set_read_only(&temporary.path)?;
        if path_is_occupied(&destination)? {
            return self.verify_cached(verified).map(|image| image.path);
        }
        fs::rename(&temporary.path, &destination).map_err(|_| ImageCacheErrorV1::Io)?;
        temporary.installed = true;
        sync_directory(&self.root)?;
        Ok(destination)
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
