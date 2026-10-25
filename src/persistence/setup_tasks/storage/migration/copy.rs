use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use fs2::FileExt;
use sha2::{Digest, Sha256};

use crate::persistence::setup_tasks::storage::permissions;

const MAX_MIGRATED_BYTES: u64 = 16 * 1024 * 1024;
const MAX_MIGRATED_FILES: usize = 2048;

#[derive(PartialEq, Eq)]
pub(super) struct TreeDigest {
    pub(super) hash: String,
    pub(super) files: usize,
    pub(super) bytes: u64,
}

pub(super) fn ensure_no_active_owners(directory: &Path) -> anyhow::Result<()> {
    let instances = directory.join("instances");
    match fs::symlink_metadata(&instances) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
        Ok(metadata) if metadata.is_dir() && !permissions::is_link_or_reparse(&metadata) => {}
        _ => anyhow::bail!("unsafe legacy machine setup owner directory"),
    }
    for entry in fs::read_dir(instances)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        anyhow::ensure!(
            metadata.is_file() && !permissions::is_link_or_reparse(&metadata),
            "unsafe legacy machine setup owner entry"
        );
        let file = open_existing_file(&entry.path(), true, true)?;
        let opened_metadata = file.metadata()?;
        anyhow::ensure!(
            opened_metadata.is_file() && !permissions::is_link_or_reparse(&opened_metadata),
            "unsafe legacy machine setup owner entry"
        );
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => FileExt::unlock(&file)?,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                anyhow::bail!("a Kool.ad/e instance still owns the legacy machine setup store")
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(super) fn copy_tree(source: &Path, destination: &Path) -> anyhow::Result<TreeDigest> {
    let mut hasher = Sha256::new();
    let mut files = 0;
    let mut bytes = 0;
    copy_directory(
        source,
        destination,
        source,
        &mut hasher,
        &mut files,
        &mut bytes,
    )?;
    Ok(TreeDigest {
        hash: format!("{:x}", hasher.finalize()),
        files,
        bytes,
    })
}

fn copy_directory(
    source: &Path,
    destination: &Path,
    root: &Path,
    hasher: &mut Sha256,
    files: &mut usize,
    bytes: &mut u64,
) -> anyhow::Result<()> {
    let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let source_path = entry.path();
        let metadata = fs::symlink_metadata(&source_path)?;
        anyhow::ensure!(
            !permissions::is_link_or_reparse(&metadata),
            "legacy machine setup state contains a symlink or reparse point"
        );
        if is_transient_entry(root, &source_path)? {
            continue;
        }
        let destination_path = destination.join(entry.file_name());
        if metadata.is_dir() {
            hasher.update(b"D");
            hasher.update(source_path.strip_prefix(root)?.to_string_lossy().as_bytes());
            fs::create_dir(&destination_path)?;
            permissions::ensure_private_directory(&destination_path)?;
            copy_directory(&source_path, &destination_path, root, hasher, files, bytes)?;
        } else {
            anyhow::ensure!(
                metadata.is_file(),
                "legacy machine setup state contains an unsupported filesystem entry"
            );
            *files += 1;
            anyhow::ensure!(
                *files <= MAX_MIGRATED_FILES,
                "legacy machine setup state has too many files"
            );
            let relative = source_path.strip_prefix(root)?.to_string_lossy();
            hasher.update(b"F");
            hasher.update(relative.as_bytes());
            copy_file(&source_path, &destination_path, hasher, bytes)?;
        }
    }
    Ok(())
}

fn copy_file(
    source: &Path,
    destination: &Path,
    hasher: &mut Sha256,
    total: &mut u64,
) -> anyhow::Result<()> {
    let mut input = open_existing_file(source, true, false)?;
    let metadata = input.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && !permissions::is_link_or_reparse(&metadata),
        "legacy setup file changed during migration"
    );

    let mut destination_options = OpenOptions::new();
    destination_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        destination_options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let mut output = destination_options.open(destination)?;
    let mut buffer = [0u8; 8192];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        *total = total
            .checked_add(count as u64)
            .ok_or_else(|| anyhow::anyhow!("legacy machine setup state size overflow"))?;
        anyhow::ensure!(
            *total <= MAX_MIGRATED_BYTES,
            "legacy machine setup state is oversized"
        );
        hasher.update(&buffer[..count]);
        output.write_all(&buffer[..count])?;
    }
    output.sync_all()?;
    drop(output);
    permissions::secure_private_file(destination)
}

pub(super) fn digest_tree(root: &Path) -> anyhow::Result<TreeDigest> {
    let mut hasher = Sha256::new();
    let mut files = 0;
    let mut bytes = 0;
    digest_directory(root, root, &mut hasher, &mut files, &mut bytes)?;
    Ok(TreeDigest {
        hash: format!("{:x}", hasher.finalize()),
        files,
        bytes,
    })
}

fn digest_directory(
    root: &Path,
    directory: &Path,
    hasher: &mut Sha256,
    files: &mut usize,
    bytes: &mut u64,
) -> anyhow::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        anyhow::ensure!(
            !permissions::is_link_or_reparse(&metadata),
            "machine setup staging state contains a symlink or reparse point"
        );
        if is_transient_entry(root, &path)? {
            continue;
        }
        if metadata.is_dir() {
            hasher.update(b"D");
            hasher.update(path.strip_prefix(root)?.to_string_lossy().as_bytes());
            digest_directory(root, &path, hasher, files, bytes)?;
        } else {
            anyhow::ensure!(
                metadata.is_file(),
                "machine setup staging has an unsupported filesystem entry"
            );
            *files += 1;
            anyhow::ensure!(
                *files <= MAX_MIGRATED_FILES,
                "machine setup state has too many files"
            );
            let relative = path.strip_prefix(root)?.to_string_lossy();
            hasher.update(b"F");
            hasher.update(relative.as_bytes());
            let mut file = open_existing_file(&path, true, false)?;
            let opened_metadata = file.metadata()?;
            anyhow::ensure!(
                opened_metadata.is_file() && !permissions::is_link_or_reparse(&opened_metadata),
                "machine setup file changed while hashing"
            );
            let mut buffer = [0u8; 8192];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                *bytes = bytes
                    .checked_add(count as u64)
                    .ok_or_else(|| anyhow::anyhow!("machine setup state size overflow"))?;
                anyhow::ensure!(
                    *bytes <= MAX_MIGRATED_BYTES,
                    "machine setup state is oversized"
                );
                hasher.update(&buffer[..count]);
            }
        }
    }
    Ok(())
}

fn is_transient_entry(root: &Path, path: &Path) -> anyhow::Result<bool> {
    let relative = path.strip_prefix(root)?;
    Ok(relative == Path::new("tasks.lock") || relative.starts_with("instances"))
}

fn open_existing_file(path: &Path, read: bool, write: bool) -> anyhow::Result<File> {
    let mut options = OpenOptions::new();
    options.read(read).write(write);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    Ok(options.open(path)?)
}

pub(super) fn remove_tree_no_links(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        !permissions::is_link_or_reparse(&metadata),
        "unsafe stale machine setup migration staging path"
    );
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            remove_tree_no_links(&entry?.path())?;
        }
        fs::remove_dir(path)?;
    } else {
        anyhow::ensure!(
            metadata.is_file(),
            "stale migration staging contains an unsupported filesystem entry"
        );
        fs::remove_file(path)?;
    }
    Ok(())
}
