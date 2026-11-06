use std::{
    ffi::OsStr,
    fs::{self, File},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use ed25519_dalek::VerifyingKey;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::computer_use::ComputerSessionV1;

use super::{
    ComputerGuestArchitectureV1, GuestImageManifestTrustV1, ImageCacheErrorV1,
    cache::{CachedGuestImageV1, GuestImageCacheV1},
    manifest::verify_guest_image_manifest,
    process::{QemuImageToolV1, QemuProcessError, run_qemu},
};

mod types;
pub use types::{GuestImageOverlayErrorV1, GuestImageOverlayRequestV1, GuestImageOverlayV1};

struct OverlayCreationCleanup {
    path: PathBuf,
    keep: bool,
}

impl Drop for OverlayCreationCleanup {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Deserialize)]
struct QemuImageInfo {
    format: String,
    #[serde(rename = "virtual-size")]
    virtual_size: u64,
    #[serde(rename = "backing-filename")]
    backing_filename: Option<String>,
    #[serde(rename = "backing-filename-format")]
    backing_format: Option<String>,
}

/// Re-verify signed catalog, current session lease, revocation, and architecture
/// immediately before creating a disposable overlay from the approved cache.
pub fn create_guest_image_overlay(
    cache: &GuestImageCacheV1,
    request: GuestImageOverlayRequestV1<'_>,
) -> Result<GuestImageOverlayV1, GuestImageOverlayErrorV1> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| GuestImageOverlayErrorV1::ManifestRejected(ImageCacheErrorV1::CatalogExpired))?
        .as_millis()
        .try_into()
        .map_err(|_| {
            GuestImageOverlayErrorV1::ManifestRejected(ImageCacheErrorV1::CatalogExpired)
        })?;
    create_guest_image_overlay_at(
        cache,
        request.manifest_bytes,
        request.trust,
        request.session,
        now,
        request.expected_architecture,
        request.controller_key_id,
        request.controller_key,
        request.generation_floor,
        request.overlay_root,
        request.qemu_tool,
    )
}

#[allow(clippy::too_many_arguments)]
fn create_guest_image_overlay_at(
    cache: &GuestImageCacheV1,
    manifest_bytes: &[u8],
    trust: &GuestImageManifestTrustV1,
    session: &ComputerSessionV1,
    now_unix_ms: u64,
    expected_architecture: ComputerGuestArchitectureV1,
    controller_key_id: &str,
    controller_key: &VerifyingKey,
    generation_floor: &super::GuestImageCatalogGenerationFloorV1,
    overlay_root: &Path,
    qemu_tool: &QemuImageToolV1,
) -> Result<GuestImageOverlayV1, GuestImageOverlayErrorV1> {
    let verified = verify_guest_image_manifest(
        manifest_bytes,
        trust,
        session,
        now_unix_ms,
        expected_architecture,
        controller_key_id,
        controller_key,
    )
    .map_err(GuestImageOverlayErrorV1::ManifestRejected)?;
    generation_floor
        .accept_generation(verified.catalog_generation())
        .map_err(|error| match error {
            ImageCacheErrorV1::CatalogRollback => GuestImageOverlayErrorV1::CatalogRollback,
            _ => GuestImageOverlayErrorV1::CatalogFloorUnavailable,
        })?;
    let image = cache
        .verify_cached(&verified)
        .map_err(|_| GuestImageOverlayErrorV1::BaseImageChanged)?;
    let session_id = session.identity.session_id.as_str();
    if !is_canonical_uuid(session_id) {
        return Err(GuestImageOverlayErrorV1::InvalidSessionId);
    }
    validate_private_directory(overlay_root)?;
    validate_base_image(&image)?;
    set_read_only(image.path())?;
    let canonical_base =
        fs::canonicalize(image.path()).map_err(|_| GuestImageOverlayErrorV1::UnsafePath)?;
    let overlay_path = overlay_root.join(format!("{}.qcow2", Uuid::new_v4()));
    if fs::symlink_metadata(&overlay_path).is_ok() {
        return Err(GuestImageOverlayErrorV1::UnsafePath);
    }
    let mut cleanup = OverlayCreationCleanup {
        path: overlay_path.clone(),
        keep: false,
    };
    let created = run_qemu(
        qemu_tool,
        &[
            OsStr::new("create"),
            OsStr::new("-f"),
            OsStr::new("qcow2"),
            OsStr::new("-F"),
            OsStr::new("qcow2"),
            OsStr::new("-b"),
            canonical_base.as_os_str(),
            overlay_path.as_os_str(),
        ],
    )
    .map_err(map_qemu_error)?;
    if !created.status.success() {
        return Err(GuestImageOverlayErrorV1::QemuImageFailed);
    }
    let metadata = fs::symlink_metadata(&overlay_path)
        .map_err(|_| GuestImageOverlayErrorV1::InvalidOverlay)?;
    if !metadata.file_type().is_file() || metadata.len() == 0 {
        return Err(GuestImageOverlayErrorV1::InvalidOverlay);
    }
    set_private_file(&overlay_path)?;
    let info = run_qemu(
        qemu_tool,
        &[
            OsStr::new("info"),
            OsStr::new("--output=json"),
            overlay_path.as_os_str(),
        ],
    )
    .map_err(map_qemu_error)?;
    if !info.status.success() {
        return Err(GuestImageOverlayErrorV1::QemuImageFailed);
    }
    let decoded: QemuImageInfo = serde_json::from_slice(&info.stdout)
        .map_err(|_| GuestImageOverlayErrorV1::InvalidOverlay)?;
    if decoded.format != "qcow2"
        || decoded.virtual_size != image.entry().image_size_bytes
        || decoded.backing_format.as_deref() != Some("qcow2")
        || decoded
            .backing_filename
            .as_deref()
            .is_none_or(|backing| Path::new(backing) != canonical_base)
    {
        return Err(GuestImageOverlayErrorV1::InvalidOverlay);
    }
    File::open(overlay_root)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| GuestImageOverlayErrorV1::Io)?;
    cleanup.keep = true;
    Ok(GuestImageOverlayV1 {
        session_id: session_id.to_owned(),
        path: overlay_path,
        directory: overlay_root.to_path_buf(),
    })
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_guest_image_overlay_at_for_test(
    cache: &GuestImageCacheV1,
    manifest_bytes: &[u8],
    trust: &GuestImageManifestTrustV1,
    session: &ComputerSessionV1,
    now_unix_ms: u64,
    expected_architecture: ComputerGuestArchitectureV1,
    controller_key_id: &str,
    controller_key: &VerifyingKey,
    generation_floor: &super::GuestImageCatalogGenerationFloorV1,
    overlay_root: &Path,
    qemu_tool: &QemuImageToolV1,
) -> Result<GuestImageOverlayV1, GuestImageOverlayErrorV1> {
    create_guest_image_overlay_at(
        cache,
        manifest_bytes,
        trust,
        session,
        now_unix_ms,
        expected_architecture,
        controller_key_id,
        controller_key,
        generation_floor,
        overlay_root,
        qemu_tool,
    )
}

fn map_qemu_error(error: QemuProcessError) -> GuestImageOverlayErrorV1 {
    match error {
        QemuProcessError::Unavailable => GuestImageOverlayErrorV1::QemuImageUnavailable,
        QemuProcessError::TimedOut | QemuProcessError::OutputTooLarge => {
            GuestImageOverlayErrorV1::QemuImageFailed
        }
        QemuProcessError::Failed => GuestImageOverlayErrorV1::QemuImageFailed,
    }
}

fn validate_base_image(image: &CachedGuestImageV1) -> Result<(), GuestImageOverlayErrorV1> {
    let metadata =
        fs::symlink_metadata(image.path()).map_err(|_| GuestImageOverlayErrorV1::UnsafePath)?;
    if !metadata.file_type().is_file() || metadata.len() != image.entry().artifact_size_bytes {
        return Err(GuestImageOverlayErrorV1::BaseImageChanged);
    }
    let mut file = File::open(image.path()).map_err(|_| GuestImageOverlayErrorV1::Io)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        use std::io::Read;
        let count = file
            .read(&mut buffer)
            .map_err(|_| GuestImageOverlayErrorV1::Io)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    if format!("sha256:{:x}", digest.finalize()) != image.entry().image_digest {
        return Err(GuestImageOverlayErrorV1::BaseImageChanged);
    }
    Ok(())
}

fn validate_private_directory(path: &Path) -> Result<(), GuestImageOverlayErrorV1> {
    fs::create_dir_all(path).map_err(|_| GuestImageOverlayErrorV1::Io)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| GuestImageOverlayErrorV1::UnsafePath)?;
    if !metadata.file_type().is_dir() {
        return Err(GuestImageOverlayErrorV1::UnsafePath);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| GuestImageOverlayErrorV1::Io)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(GuestImageOverlayErrorV1::UnsafePath)
    }
}

fn set_read_only(path: &Path) -> Result<(), GuestImageOverlayErrorV1> {
    let mut permissions = fs::metadata(path)
        .map_err(|_| GuestImageOverlayErrorV1::Io)?
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).map_err(|_| GuestImageOverlayErrorV1::Io)
}

fn set_private_file(path: &Path) -> Result<(), GuestImageOverlayErrorV1> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|_| GuestImageOverlayErrorV1::Io)?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn is_canonical_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}
