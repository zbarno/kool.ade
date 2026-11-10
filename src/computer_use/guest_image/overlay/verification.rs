use std::{
    ffi::OsStr,
    fs::{self, File},
    path::Path,
};

use sha2::{Digest, Sha256};

use super::super::cache::CachedGuestImageV1;
use super::super::process::{QemuImageToolV1, run_qemu};
use super::{
    GuestImageOverlayErrorV1, GuestImageOverlayV1, QemuImageInfo, map_qemu_error,
    validate_private_directory,
};

pub fn verify_overlay_before_boot(
    overlay: &GuestImageOverlayV1,
) -> Result<(), GuestImageOverlayErrorV1> {
    let tool = super::super::process::probe_host_qemu_image_tool().map_err(map_qemu_error)?;
    verify_overlay_before_boot_with_tool(overlay, &tool)
}

pub fn verify_guest_image_overlay_before_boot(
    overlay: &GuestImageOverlayV1,
) -> Result<(), GuestImageOverlayErrorV1> {
    verify_overlay_before_boot(overlay)
}

pub(crate) fn verify_overlay_before_boot_with_tool(
    overlay: &GuestImageOverlayV1,
    tool: &QemuImageToolV1,
) -> Result<(), GuestImageOverlayErrorV1> {
    let directory = &overlay.directory;
    validate_private_directory(directory)?;
    if overlay.path.parent() != Some(directory.as_path()) {
        return Err(GuestImageOverlayErrorV1::UnsafePath);
    }
    let metadata = fs::symlink_metadata(&overlay.path)
        .map_err(|_| GuestImageOverlayErrorV1::InvalidOverlay)?;
    if !metadata.file_type().is_file() {
        return Err(GuestImageOverlayErrorV1::InvalidOverlay);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(GuestImageOverlayErrorV1::UnsafePath);
        }
    }
    validate_base_image_parts(
        &overlay.base_image_path,
        overlay.base_artifact_size_bytes,
        &overlay.image_digest,
    )?;
    let info = run_qemu(
        tool,
        &[
            OsStr::new("info"),
            OsStr::new("--output=json"),
            overlay.path.as_os_str(),
        ],
    )
    .map_err(map_qemu_error)?;
    if !info.status.success() {
        return Err(GuestImageOverlayErrorV1::QemuImageFailed);
    }
    let decoded: QemuImageInfo = serde_json::from_slice(&info.stdout)
        .map_err(|_| GuestImageOverlayErrorV1::InvalidOverlay)?;
    let canonical_base = fs::canonicalize(&overlay.base_image_path)
        .map_err(|_| GuestImageOverlayErrorV1::UnsafePath)?;
    if overlay.persistent_disk_key.is_some() {
        if decoded.format != "qcow2"
            || decoded.virtual_size != overlay.virtual_size_bytes
            || decoded.backing_filename.is_some()
            || decoded.encrypted != Some(true)
            || decoded.format_specific.is_none_or(|specific| {
                specific.format_type != "qcow2"
                    || specific
                        .data
                        .encrypt
                        .is_none_or(|encryption| encryption.format != "luks")
            })
        {
            return Err(GuestImageOverlayErrorV1::InvalidOverlay);
        }
        return Ok(());
    }
    if decoded.format != "qcow2"
        || decoded.virtual_size != overlay.virtual_size_bytes
        || decoded.backing_format.as_deref() != Some("qcow2")
        || decoded
            .backing_filename
            .as_deref()
            .is_none_or(|backing| Path::new(backing) != canonical_base)
    {
        return Err(GuestImageOverlayErrorV1::InvalidOverlay);
    }
    Ok(())
}

pub(super) fn validate_base_image(
    image: &CachedGuestImageV1,
) -> Result<(), GuestImageOverlayErrorV1> {
    validate_base_image_parts(
        image.path(),
        image.entry().artifact_size_bytes,
        &image.entry().image_digest,
    )
}

fn validate_base_image_parts(
    path: &Path,
    expected_size: u64,
    expected_digest: &str,
) -> Result<(), GuestImageOverlayErrorV1> {
    let metadata = fs::symlink_metadata(path).map_err(|_| GuestImageOverlayErrorV1::UnsafePath)?;
    if !metadata.file_type().is_file() || metadata.len() != expected_size {
        return Err(GuestImageOverlayErrorV1::BaseImageChanged);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o222 != 0
        {
            return Err(GuestImageOverlayErrorV1::BaseImageChanged);
        }
    }
    let mut file = File::open(path).map_err(|_| GuestImageOverlayErrorV1::Io)?;
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
    if format!("sha256:{:x}", digest.finalize()) != expected_digest {
        return Err(GuestImageOverlayErrorV1::BaseImageChanged);
    }
    Ok(())
}
