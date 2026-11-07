use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use reqwest::StatusCode;
use uuid::Uuid;

#[cfg(test)]
use super::super::manifest::VerifiedGuestImageV1;
use super::super::{
    floor::GuestImageCatalogGenerationFloorV1,
    manifest::{
        ComputerGuestArchitectureV1, GuestImageEntryV1, GuestImageManifestTrustV1,
        VerifiedCatalogGuestImageV1, verify_guest_image_catalog,
    },
};
use super::{
    CachedGuestImageV1, ComputerImageDownloadApprovalV1, GuestImageCacheV1, ImageCacheErrorV1,
    storage::{copy_bounded, path_is_occupied, set_read_only, sync_directory, verify_file},
    transport::{release_client, release_tag},
};

const RELEASE_BASE: &str = "https://github.com/zbarno/kool.ade/releases/download";

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
    /// Download a catalog-approved image before a session exists. This proves
    /// only catalog authorization; callers must still use session-bound
    /// verification before creating an overlay or starting a VM.
    pub fn download_approved_catalog_release(
        &self,
        catalog_bytes: &[u8],
        trust: &GuestImageManifestTrustV1,
        generation_floor: &GuestImageCatalogGenerationFloorV1,
        architecture: ComputerGuestArchitectureV1,
        approval: &dyn ComputerImageDownloadApprovalV1,
    ) -> Result<CachedGuestImageV1, ImageCacheErrorV1> {
        let verified = verify_current_catalog_image(
            catalog_bytes,
            trust,
            architecture,
            current_time_unix_ms()?,
        )?;
        approval.authorize_download(&verified.manifest_digest, &verified.entry.image_digest)?;
        generation_floor.accept_generation(verified.catalog_generation)?;
        let path = self.image_path(&verified.entry)?;
        if path.exists() {
            verify_file(&path, &verified.entry)?;
            return Ok(cached_catalog_image(&verified, path));
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
        let path = self.install_catalog_reader_authorized(&verified, &mut response)?;
        Ok(cached_catalog_image(&verified, path))
    }

    /// Streaming seam for deterministic tests and the bounded HTTP client.
    #[cfg(test)]
    pub(crate) fn install_approved_reader<R: Read>(
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
        self.install_entry_reader_authorized(&verified.entry, None, reader)
            .map(|path| CachedGuestImageV1 {
                entry: verified.entry.clone(),
                manifest_digest: verified.manifest_digest.clone(),
                manifest_signer_key_id: verified.manifest_signer_key_id.clone(),
                catalog_generation: verified.catalog_generation,
                path,
            })
    }

    /// Streaming seam for catalog-approved pre-session downloads.
    #[cfg(test)]
    pub(crate) fn install_approved_catalog_reader<R: Read>(
        &self,
        verified: &VerifiedCatalogGuestImageV1,
        generation_floor: &GuestImageCatalogGenerationFloorV1,
        now_unix_ms: u64,
        approval: &dyn ComputerImageDownloadApprovalV1,
        reader: &mut R,
    ) -> Result<CachedGuestImageV1, ImageCacheErrorV1> {
        ensure_catalog_current(verified, now_unix_ms)?;
        approval.authorize_download(&verified.manifest_digest, &verified.entry.image_digest)?;
        generation_floor.accept_generation(verified.catalog_generation)?;
        let destination = self.image_path(&verified.entry)?;
        if path_is_occupied(&destination)? {
            verify_file(&destination, &verified.entry)?;
            return Ok(cached_catalog_image(verified, destination));
        }
        let path = self.install_entry_reader_authorized(&verified.entry, None, reader)?;
        Ok(cached_catalog_image(verified, path))
    }

    fn install_catalog_reader_authorized<R: Read>(
        &self,
        verified: &VerifiedCatalogGuestImageV1,
        reader: &mut R,
    ) -> Result<PathBuf, ImageCacheErrorV1> {
        self.install_entry_reader_authorized(
            &verified.entry,
            Some(verified.expires_at_unix_ms),
            reader,
        )
    }

    fn install_entry_reader_authorized<R: Read>(
        &self,
        entry: &GuestImageEntryV1,
        expires_at_unix_ms: Option<u64>,
        reader: &mut R,
    ) -> Result<PathBuf, ImageCacheErrorV1> {
        let destination = self.image_path(entry)?;
        if path_is_occupied(&destination)? {
            verify_file(&destination, entry)?;
            return Ok(destination);
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
        copy_bounded(reader, &mut file, entry)?;
        file.flush().map_err(|_| ImageCacheErrorV1::Io)?;
        file.sync_all().map_err(|_| ImageCacheErrorV1::Io)?;
        set_read_only(&temporary.path)?;
        if let Some(expires_at_unix_ms) = expires_at_unix_ms {
            ensure_expiry(expires_at_unix_ms)?;
        }
        if path_is_occupied(&destination)? {
            verify_file(&destination, entry)?;
            return Ok(destination);
        }
        fs::rename(&temporary.path, &destination).map_err(|_| ImageCacheErrorV1::Io)?;
        temporary.installed = true;
        sync_directory(&self.root)?;
        Ok(destination)
    }
}

#[cfg(test)]
fn ensure_catalog_current(
    verified: &VerifiedCatalogGuestImageV1,
    now_unix_ms: u64,
) -> Result<(), ImageCacheErrorV1> {
    ensure_expiry_at(verified.expires_at_unix_ms, now_unix_ms)
}

pub(super) fn ensure_expiry(expires_at_unix_ms: u64) -> Result<(), ImageCacheErrorV1> {
    ensure_expiry_at(expires_at_unix_ms, current_time_unix_ms()?)
}

pub(super) fn current_time_unix_ms() -> Result<u64, ImageCacheErrorV1> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ImageCacheErrorV1::CatalogExpired)?
        .as_millis()
        .try_into()
        .map_err(|_| ImageCacheErrorV1::CatalogExpired)
}

fn verify_current_catalog_image(
    catalog_bytes: &[u8],
    trust: &GuestImageManifestTrustV1,
    architecture: ComputerGuestArchitectureV1,
    now_unix_ms: u64,
) -> Result<VerifiedCatalogGuestImageV1, ImageCacheErrorV1> {
    verify_guest_image_catalog(catalog_bytes, trust, now_unix_ms)?
        .approved_image_for_download(architecture)
        .ok_or(ImageCacheErrorV1::ImageNotCatalogued)
}

fn ensure_expiry_at(expires_at_unix_ms: u64, now_unix_ms: u64) -> Result<(), ImageCacheErrorV1> {
    if now_unix_ms >= expires_at_unix_ms {
        return Err(ImageCacheErrorV1::CatalogExpired);
    }
    Ok(())
}

fn cached_catalog_image(
    verified: &VerifiedCatalogGuestImageV1,
    path: PathBuf,
) -> CachedGuestImageV1 {
    CachedGuestImageV1 {
        entry: verified.entry.clone(),
        manifest_digest: verified.manifest_digest.clone(),
        manifest_signer_key_id: verified.manifest_signer_key_id.clone(),
        catalog_generation: verified.catalog_generation,
        path,
    }
}
