use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    DependencyCacheSnapshotStore, MAX_STORED_BYTES, MAX_STORED_HEADER_BYTES, MAX_STORED_SNAPSHOTS,
};

const LAST_USED_FILE: &str = ".last-used";

struct SnapshotEntry {
    directory: PathBuf,
    last_used_millis: u64,
    stored_bytes: u64,
}

impl DependencyCacheSnapshotStore {
    pub(super) fn ensure_capacity_for(
        &self,
        incoming_archive_bytes: usize,
        incoming_header_bytes: usize,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            incoming_header_bytes <= MAX_STORED_HEADER_BYTES,
            "cache snapshot metadata exceeds its size limit"
        );
        let incoming_bytes = (incoming_archive_bytes as u64)
            .saturating_add(incoming_header_bytes as u64)
            .saturating_add(8);
        anyhow::ensure!(
            incoming_bytes <= MAX_STORED_BYTES,
            "cache snapshot exceeds the total retained cache limit"
        );

        let mut entries = Vec::new();
        let mut total_bytes = 0u64;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".lock" {
                continue;
            }
            if name.starts_with(".staging-") {
                fs::remove_dir_all(&path)?;
                continue;
            }

            super::ensure_real_directory(&path)?;
            remove_abandoned_markers(&path)?;
            validate_snapshot_contents(&path)?;
            let manifest_bytes = regular_file_len(&path.join("manifest.json"))?;
            let archive_bytes = regular_file_len(&path.join("snapshot.tar"))?;
            let (last_used_millis, marker_bytes) = match read_last_used(&path)? {
                Some(value) => (value, 8),
                None => (directory_modified_millis(&path)?, 0),
            };
            let stored_bytes = manifest_bytes
                .saturating_add(archive_bytes)
                .saturating_add(marker_bytes);
            total_bytes = total_bytes.saturating_add(stored_bytes);
            entries.push(SnapshotEntry {
                directory: path,
                last_used_millis,
                stored_bytes,
            });
        }

        for directory in eviction_candidates(entries, total_bytes, incoming_bytes)? {
            fs::remove_dir_all(directory)?;
        }
        Ok(())
    }
}

fn eviction_candidates(
    mut entries: Vec<SnapshotEntry>,
    mut total_bytes: u64,
    incoming_bytes: u64,
) -> anyhow::Result<Vec<PathBuf>> {
    entries.sort_by(|left, right| {
        left.last_used_millis
            .cmp(&right.last_used_millis)
            .then_with(|| left.directory.cmp(&right.directory))
    });
    let mut remaining = entries.len();
    let mut candidates = Vec::new();
    for entry in entries {
        if remaining.saturating_add(1) <= MAX_STORED_SNAPSHOTS
            && total_bytes.saturating_add(incoming_bytes) <= MAX_STORED_BYTES
        {
            break;
        }
        remaining = remaining.saturating_sub(1);
        total_bytes = total_bytes.saturating_sub(entry.stored_bytes);
        candidates.push(entry.directory);
    }
    anyhow::ensure!(
        remaining.saturating_add(1) <= MAX_STORED_SNAPSHOTS
            && total_bytes.saturating_add(incoming_bytes) <= MAX_STORED_BYTES,
        "dependency cache store cannot make room for this snapshot"
    );
    Ok(candidates)
}

pub(super) fn touch_last_used(directory: &Path) -> anyhow::Result<()> {
    remove_abandoned_markers(directory)?;
    read_last_used(directory)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64;
    let temporary = directory.join(format!(
        ".last-used-staging-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let marker = directory.join(LAST_USED_FILE);
    let result = (|| {
        let mut file = private_new_file(&temporary)?;
        file.write_all(&timestamp.to_be_bytes())?;
        file.sync_all()?;
        replace_file(&temporary, &marker)?;
        Ok::<_, anyhow::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let source = source
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let target = target
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            #[link_name = "MoveFileExW"]
            fn move_file_ex_w(existing: *const u16, new: *const u16, flags: u32) -> i32;
        }
        // SAFETY: both paths are NUL-terminated UTF-16 buffers that remain
        // alive for the call; MoveFileExW does not retain either pointer.
        if unsafe {
            move_file_ex_w(
                source.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(source, target)
    }
}

fn remove_abandoned_markers(directory: &Path) -> anyhow::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(".last-used-staging-")
        {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "cache store contains an invalid last-used staging entry"
        );
        fs::remove_file(entry.path())?;
    }
    Ok(())
}

fn validate_snapshot_contents(directory: &Path) -> anyhow::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        anyhow::ensure!(
            matches!(
                name.as_ref(),
                "manifest.json" | "snapshot.tar" | LAST_USED_FILE
            ),
            "cache store contains an unexpected snapshot entry"
        );
        let metadata = fs::symlink_metadata(entry.path())?;
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "cache store contains a non-regular snapshot file"
        );
    }
    Ok(())
}

fn regular_file_len(path: &Path) -> anyhow::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "cache store contains a non-regular snapshot file"
    );
    Ok(metadata.len())
}

fn read_last_used(directory: &Path) -> anyhow::Result<Option<u64>> {
    let path = directory.join(LAST_USED_FILE);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() == 8,
        "cache store contains an invalid last-used marker"
    );
    let bytes = fs::read(path)?;
    let timestamp: [u8; 8] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("cache store contains an invalid last-used marker"))?;
    Ok(Some(u64::from_be_bytes(timestamp)))
}

fn directory_modified_millis(directory: &Path) -> anyhow::Result<u64> {
    let modified = fs::metadata(directory)?.modified()?;
    Ok(modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64)
}

#[cfg(unix)]
fn private_new_file(path: &Path) -> anyhow::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?)
}

#[cfg(test)]
#[path = "retention/tests.rs"]
mod tests;

#[cfg(not(unix))]
fn private_new_file(path: &Path) -> anyhow::Result<fs::File> {
    Ok(fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?)
}
