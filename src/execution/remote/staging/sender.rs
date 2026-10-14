//! Bounded streaming of manifest-authorized workspace and result files.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use super::{MAX_TRANSFER_CHUNK_BYTES, TransferChunkV1, TransferManifestV1, TransferPayloadV1};

/// Stream one authorized file as bounded integrity-checked chunks. Chunks are
/// provisional until the final source digest has been verified by this call.
pub fn stream_file_chunks(
    manifest: &TransferManifestV1,
    root: &Path,
    relative_path: &str,
    mut send: impl FnMut(&TransferChunkV1) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    manifest.validate()?;
    let file = manifest
        .files()
        .into_iter()
        .find(|file| file.path == relative_path)
        .ok_or_else(|| anyhow::anyhow!("file is absent from transfer manifest"))?;
    let path = safe_source(root, relative_path)?;
    match &manifest.payload {
        TransferPayloadV1::Workspace(workspace) | TransferPayloadV1::WorkspaceResult(workspace) => {
            let mut offset = 0_u64;
            workspace
                .stream_file(root, relative_path, |block| {
                    for bytes in block.chunks(MAX_TRANSFER_CHUNK_BYTES) {
                        let chunk = TransferChunkV1::new(
                            manifest.transfer_id.clone(),
                            relative_path,
                            offset,
                            bytes.to_vec(),
                        )
                        .map_err(|_| transfer_denied("Workspace chunk is invalid"))?;
                        send(&chunk)
                            .map_err(|_| transfer_denied("Workspace chunk could not be sent"))?;
                        offset = offset.saturating_add(bytes.len() as u64);
                    }
                    Ok(())
                })
                .map_err(|error| anyhow::anyhow!(error))?;
        }
        TransferPayloadV1::Results(_) => stream_result_file(
            manifest,
            &path,
            relative_path,
            file.size_bytes,
            file.sha256,
            &mut send,
        )?,
    }
    Ok(())
}

pub(super) fn read_file_chunk(
    manifest: &TransferManifestV1,
    root: &Path,
    relative_path: &str,
    offset: u64,
) -> anyhow::Result<TransferChunkV1> {
    let mut selected = None;
    stream_file_chunks(manifest, root, relative_path, |chunk| {
        if chunk.offset == offset {
            selected = Some(chunk.clone());
        }
        Ok(())
    })?;
    selected.ok_or_else(|| anyhow::anyhow!("requested result chunk is not in the file"))
}

fn stream_result_file(
    manifest: &TransferManifestV1,
    path: &Path,
    relative_path: &str,
    expected_size: u64,
    expected_hash: &str,
    send: &mut impl FnMut(&TransferChunkV1) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut input = options.open(path)?;
    let metadata = input.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() == expected_size,
        "result artifact changed after manifest creation"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            metadata.nlink() == 1 && metadata.mode() & 0o6000 == 0,
            "result artifact file metadata is unsafe"
        );
    }
    let mut digest = Sha256::new();
    let mut buffer = vec![0; MAX_TRANSFER_CHUNK_BYTES];
    let mut offset = 0_u64;
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        offset = offset
            .checked_add(count as u64)
            .ok_or_else(|| anyhow::anyhow!("result file size overflow"))?;
        anyhow::ensure!(
            offset <= expected_size,
            "result artifact grew during transfer"
        );
        digest.update(&buffer[..count]);
        let chunk = TransferChunkV1::new(
            manifest.transfer_id.clone(),
            relative_path,
            offset - count as u64,
            buffer[..count].to_vec(),
        )?;
        send(&chunk)?;
    }
    anyhow::ensure!(
        offset == expected_size && format!("{:x}", digest.finalize()) == expected_hash,
        "result artifact changed during transfer"
    );
    Ok(())
}

fn safe_source(root: &Path, relative_path: &str) -> anyhow::Result<PathBuf> {
    super::manifest::validate_transfer_path(relative_path)?;
    let metadata = fs::symlink_metadata(root)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "transfer source root is not trusted"
    );
    let mut path = root.to_path_buf();
    let components = relative_path.split('/').collect::<Vec<_>>();
    for component in &components[..components.len().saturating_sub(1)] {
        anyhow::ensure!(
            !component.is_empty() && *component != "." && *component != "..",
            "transfer path is not normalized"
        );
        path.push(component);
        let metadata = fs::symlink_metadata(&path)?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "transfer parent is not a real directory"
        );
    }
    let file = components
        .last()
        .ok_or_else(|| anyhow::anyhow!("transfer path is empty"))?;
    anyhow::ensure!(
        !file.is_empty() && *file != "." && *file != "..",
        "transfer path is not normalized"
    );
    path.push(file);
    Ok(path)
}

fn transfer_denied(message: &'static str) -> crate::execution::errors::ExecutionError {
    crate::execution::errors::ExecutionError::new(
        crate::execution::errors::ExecutionErrorKind::PolicyDenied,
        message,
        false,
    )
}

#[cfg(test)]
#[path = "sender/tests.rs"]
mod tests;
