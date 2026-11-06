use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::cache::CachedGuestImageV1;

#[derive(Debug, Eq, PartialEq)]
pub enum GuestImageOverlayErrorV1 {
    InvalidSessionId,
    UnsafePath,
    BaseImageChanged,
    QemuImageUnavailable,
    QemuImageFailed,
    InvalidOverlay,
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
    session_id: String,
    path: PathBuf,
    directory: PathBuf,
}

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

impl GuestImageOverlayV1 {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The VM backend must stop the VM before releasing its disposable overlay.
    pub fn release_after_vm_stopped(self) -> Result<(), GuestImageOverlayErrorV1> {
        validate_private_directory(&self.directory)?;
        let metadata =
            fs::symlink_metadata(&self.path).map_err(|_| GuestImageOverlayErrorV1::Io)?;
        if !metadata.file_type().is_file() || self.path.parent() != Some(self.directory.as_path()) {
            return Err(GuestImageOverlayErrorV1::UnsafePath);
        }
        fs::remove_file(&self.path).map_err(|_| GuestImageOverlayErrorV1::Io)
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

/// Build one qcow2 overlay from a previously verified, immutable image.
/// `qemu_img` must come from the host's trusted QEMU probe; no session or task
/// data is used as an executable path or command argument.
pub fn create_guest_image_overlay(
    image: &CachedGuestImageV1,
    session_id: &str,
    overlay_root: &Path,
    qemu_img: &Path,
) -> Result<GuestImageOverlayV1, GuestImageOverlayErrorV1> {
    if !is_canonical_uuid(session_id) {
        return Err(GuestImageOverlayErrorV1::InvalidSessionId);
    }
    validate_private_directory(overlay_root)?;
    validate_base_image(image)?;
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
    let created = Command::new(qemu_img)
        .args(["create", "-f", "qcow2", "-F", "qcow2", "-b"])
        .arg(&canonical_base)
        .arg(&overlay_path)
        .output()
        .map_err(|_| GuestImageOverlayErrorV1::QemuImageUnavailable)?;
    if !created.status.success() {
        return Err(GuestImageOverlayErrorV1::QemuImageFailed);
    }
    let metadata = fs::symlink_metadata(&overlay_path)
        .map_err(|_| GuestImageOverlayErrorV1::InvalidOverlay)?;
    if !metadata.file_type().is_file() || metadata.len() == 0 {
        return Err(GuestImageOverlayErrorV1::InvalidOverlay);
    }
    set_private_file(&overlay_path)?;
    let info = Command::new(qemu_img)
        .args(["info", "--output=json"])
        .arg(&overlay_path)
        .output()
        .map_err(|_| GuestImageOverlayErrorV1::QemuImageUnavailable)?;
    if !info.status.success() {
        return Err(GuestImageOverlayErrorV1::QemuImageFailed);
    }
    let decoded: QemuImageInfo = serde_json::from_slice(&info.stdout)
        .map_err(|_| GuestImageOverlayErrorV1::InvalidOverlay)?;
    if decoded.format != "qcow2"
        || decoded.virtual_size != image.entry().image_size_bytes
        || decoded.backing_format.as_deref() != Some("qcow2")
        || decoded.backing_filename.as_deref() != Some(canonical_base.to_string_lossy().as_ref())
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
    Ok(())
}

fn is_canonical_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}
