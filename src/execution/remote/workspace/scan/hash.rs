use std::{fs, io::Read, path::Path};

use sha2::{Digest, Sha256};

use crate::execution::errors::ExecutionError;

pub(super) fn hash_file(path: &Path, expected_size: u64) -> Result<(String, bool), ExecutionError> {
    stream_file(path, expected_size, |_| Ok(()))
}

pub(super) fn stream_file(
    path: &Path,
    expected_size: u64,
    mut on_chunk: impl FnMut(&[u8]) -> Result<(), ExecutionError>,
) -> Result<(String, bool), ExecutionError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut file = options
        .open(path)
        .map_err(|_| super::denied("Task clone file could not be opened safely"))?;
    let before = file
        .metadata()
        .map_err(|_| super::denied("Task clone file could not be inspected"))?;
    verify_regular_file(&before)
        .map_err(|_| super::denied("Task clone file is not a regular file"))?;
    if before.len() != expected_size {
        return Err(super::denied("Task clone file changed during snapshot"));
    }
    let mut digest = Sha256::new();
    let mut length = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| super::denied("Task clone file could not be read"))?;
        if read == 0 {
            break;
        }
        length = length.saturating_add(read as u64);
        if length > expected_size {
            return Err(super::denied("Task clone file grew during snapshot"));
        }
        digest.update(&buffer[..read]);
        on_chunk(&buffer[..read])?;
    }
    if length != expected_size {
        return Err(super::denied("Task clone file shrank during snapshot"));
    }
    let after = file
        .metadata()
        .map_err(|_| super::denied("Task clone file could not be reinspected"))?;
    if after.len() != expected_size {
        return Err(super::denied("Task clone file changed during snapshot"));
    }
    Ok((format!("{:x}", digest.finalize()), is_executable(&after)))
}

pub(super) fn verify_regular_file(metadata: &fs::Metadata) -> anyhow::Result<()> {
    anyhow::ensure!(metadata.is_file() && !metadata.file_type().is_symlink());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(metadata.nlink() == 1);
        anyhow::ensure!(metadata.mode() & 0o6000 == 0);
        anyhow::ensure!(metadata.blocks().saturating_mul(512) >= metadata.len());
    }
    Ok(())
}

fn is_executable(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}
