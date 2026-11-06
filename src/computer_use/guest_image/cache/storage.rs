use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

use sha2::{Digest, Sha256};

use super::ImageCacheErrorV1;
use crate::computer_use::guest_image::manifest::GuestImageEntryV1;

pub(super) fn copy_bounded<R: Read>(
    reader: &mut R,
    output: &mut File,
    entry: &GuestImageEntryV1,
) -> Result<(), ImageCacheErrorV1> {
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| ImageCacheErrorV1::DownloadFailed)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .filter(|size| *size <= entry.artifact_size_bytes)
            .ok_or(ImageCacheErrorV1::DownloadTooLarge)?;
        output
            .write_all(&buffer[..count])
            .map_err(|_| ImageCacheErrorV1::Io)?;
        digest.update(&buffer[..count]);
    }
    if total != entry.artifact_size_bytes {
        return Err(ImageCacheErrorV1::DownloadLengthMismatch);
    }
    if format!("sha256:{:x}", digest.finalize()) != entry.image_digest {
        return Err(ImageCacheErrorV1::DigestMismatch);
    }
    Ok(())
}

pub(super) fn verify_file(path: &Path, entry: &GuestImageEntryV1) -> Result<(), ImageCacheErrorV1> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ImageCacheErrorV1::CacheCorrupt)?;
    if !metadata.file_type().is_file() || metadata.len() != entry.artifact_size_bytes {
        return Err(ImageCacheErrorV1::CacheCorrupt);
    }
    let mut file = File::open(path).map_err(|_| ImageCacheErrorV1::CacheCorrupt)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| ImageCacheErrorV1::CacheCorrupt)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    if format!("sha256:{:x}", digest.finalize()) != entry.image_digest {
        return Err(ImageCacheErrorV1::CacheCorrupt);
    }
    Ok(())
}

pub(super) fn path_is_occupied(path: &Path) -> Result<bool, ImageCacheErrorV1> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ImageCacheErrorV1::Io),
    }
}

pub(super) fn prepare_private_directory(path: &Path) -> Result<(), ImageCacheErrorV1> {
    fs::create_dir_all(path).map_err(|_| ImageCacheErrorV1::Io)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| ImageCacheErrorV1::Io)?;
    if !metadata.file_type().is_dir() {
        return Err(ImageCacheErrorV1::CachePathUnsafe);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| ImageCacheErrorV1::Io)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(ImageCacheErrorV1::CachePathUnsafe)
    }
}

pub(super) fn set_read_only(path: &Path) -> Result<(), ImageCacheErrorV1> {
    let mut permissions = fs::metadata(path)
        .map_err(|_| ImageCacheErrorV1::Io)?
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).map_err(|_| ImageCacheErrorV1::Io)
}

pub(super) fn sync_directory(path: &Path) -> Result<(), ImageCacheErrorV1> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| ImageCacheErrorV1::Io)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(ImageCacheErrorV1::CachePathUnsafe)
    }
}
