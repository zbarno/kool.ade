use std::io::Read;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::super::TransferChunkV1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct ChunkRecord {
    pub path: String,
    pub offset: u64,
    pub size_bytes: usize,
    pub sha256: String,
}

impl ChunkRecord {
    pub fn from_chunk(chunk: &TransferChunkV1) -> Self {
        Self {
            path: chunk.path.clone(),
            offset: chunk.offset,
            size_bytes: chunk.bytes.len(),
            sha256: chunk.sha256.clone(),
        }
    }
}

pub(super) fn transfer_directory(root: &Path, id: &str, create: bool) -> anyhow::Result<PathBuf> {
    let path = root.join(id);
    if create {
        fs::create_dir_all(&path)?;
        ensure_private_dir(&path)?;
    }
    let metadata = fs::symlink_metadata(&path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "transfer directory is not trusted"
    );
    Ok(path)
}

pub(super) fn ensure_private_dir(path: &Path) -> anyhow::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            std::path::Component::RootDir => current.push(component.as_os_str()),
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir => {
                anyhow::bail!("staging path contains a parent component")
            }
            std::path::Component::Normal(name) => current.push(name),
        }
        if current.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) => anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "staging path contains an untrusted directory"
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&current, fs::Permissions::from_mode(0o700))?;
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "staging directory is not trusted"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(super) fn set_private_file(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub(super) fn read_bounded(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= limit,
        "staging record is not trusted"
    );
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= limit,
        "staging record exceeds its limit"
    );
    Ok(bytes)
}

pub(super) fn path_digest(path: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(path.as_bytes()))
}

pub(super) fn chunk_path(directory: &Path, path_hash: &str, offset: u64) -> PathBuf {
    directory
        .join("chunks")
        .join(path_hash)
        .join(format!("{offset:020}.chunk"))
}

pub(super) fn prepare_chunk_directory(path: &Path) -> anyhow::Result<()> {
    ensure_private_dir(path)
}

pub(super) fn verify_chunk_file(directory: &Path, chunk: &TransferChunkV1) -> anyhow::Result<()> {
    let path = chunk_path(directory, &path_digest(&chunk.path), chunk.offset);
    anyhow::ensure!(
        read_bounded(&path, super::super::MAX_TRANSFER_CHUNK_BYTES as u64)? == chunk.bytes,
        "stored transfer chunk conflicts with replay"
    );
    Ok(())
}

pub(super) fn append_journal(directory: &Path, record: &ChunkRecord) -> anyhow::Result<()> {
    let path = directory.join("chunks.jsonl");
    let current = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata.len(),
        Ok(_) => anyhow::bail!("transfer journal is not trusted"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error.into()),
    };
    let mut encoded = serde_json::to_vec(record)?;
    encoded.push(b'\n');
    anyhow::ensure!(
        current.saturating_add(encoded.len() as u64) <= super::MAX_JOURNAL_BYTES,
        "transfer journal exceeds its limit"
    );
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut file = options.open(&path)?;
    use std::io::Write;
    file.write_all(&encoded)?;
    file.sync_data()?;
    set_private_file(&path)?;
    crate::artifacts::sync_parent_directory(&path)?;
    Ok(())
}

pub(super) fn read_journal(
    directory: &Path,
) -> anyhow::Result<std::collections::BTreeMap<(String, u64), ChunkRecord>> {
    let path = directory.join("chunks.jsonl");
    let bytes = match read_bounded(&path, super::MAX_JOURNAL_BYTES) {
        Ok(bytes) => bytes,
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::NotFound) =>
        {
            return Ok(Default::default());
        }
        Err(error) => return Err(error),
    };
    if bytes.is_empty() {
        return Ok(Default::default());
    }
    anyhow::ensure!(
        bytes.last() == Some(&b'\n'),
        "transfer journal has a truncated record"
    );
    let mut records = std::collections::BTreeMap::new();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let record: ChunkRecord = serde_json::from_slice(line)?;
        anyhow::ensure!(
            record.size_bytes > 0
                && record.size_bytes <= super::super::MAX_TRANSFER_CHUNK_BYTES
                && record.sha256.len() == 64,
            "transfer journal record is invalid"
        );
        let key = (record.path.clone(), record.offset);
        if let Some(previous) = records.insert(key, record.clone()) {
            anyhow::ensure!(previous == record, "transfer journal replay conflicts");
        }
        anyhow::ensure!(
            records.len() <= super::MAX_CHUNK_RECORDS,
            "transfer journal has too many records"
        );
    }
    Ok(records)
}

pub(super) fn open_private_lock(path: &Path) -> anyhow::Result<fs::File> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "transfer lock is not trusted"
        );
    }
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    set_private_file(path)?;
    Ok(file)
}
