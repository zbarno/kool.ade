use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use super::{GuestJournalErrorV1, JournalStateV1};

const MAX_JOURNAL_BYTES: usize = 32 * 1024 * 1024;

pub(super) fn read_state(
    path: &Path,
    session_id: &str,
    lease_generation: u64,
) -> Result<JournalStateV1, GuestJournalErrorV1> {
    let metadata = fs::symlink_metadata(path).map_err(|_| GuestJournalErrorV1::Io)?;
    if !metadata.file_type().is_file() {
        return Err(GuestJournalErrorV1::OwnershipMismatch);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(GuestJournalErrorV1::OwnershipMismatch);
        }
    }
    let mut bytes = Vec::new();
    open_read_private_file(path)?
        .take(MAX_JOURNAL_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| GuestJournalErrorV1::Io)?;
    if bytes.len() > MAX_JOURNAL_BYTES {
        return Err(GuestJournalErrorV1::JournalFull);
    }
    let state: JournalStateV1 =
        serde_json::from_slice(&bytes).map_err(|_| GuestJournalErrorV1::Corrupt)?;
    if state.schema_version != 1
        || state.session_id != session_id
        || state.lease_generation != lease_generation
        || state.entries.len() > 100_000
    {
        return Err(GuestJournalErrorV1::OwnershipMismatch);
    }
    Ok(state)
}

pub(super) fn write_state(
    directory: &Path,
    path: &Path,
    state: &JournalStateV1,
) -> Result<(), GuestJournalErrorV1> {
    let bytes = serde_json::to_vec(state).map_err(|_| GuestJournalErrorV1::Corrupt)?;
    if bytes.len() > MAX_JOURNAL_BYTES {
        return Err(GuestJournalErrorV1::JournalFull);
    }
    let temp = directory.join(format!("guest-actions.tmp.{}", uuid::Uuid::new_v4()));
    let mut file = open_new_private_file(&temp)?;
    file.write_all(&bytes)
        .map_err(|_| GuestJournalErrorV1::Io)?;
    file.sync_all().map_err(|_| GuestJournalErrorV1::Io)?;
    fs::rename(&temp, path).map_err(|_| GuestJournalErrorV1::Io)?;
    File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| GuestJournalErrorV1::Io)
}

pub(super) fn open_lock_file(path: &Path) -> Result<File, GuestJournalErrorV1> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options.open(path).map_err(|_| GuestJournalErrorV1::Io)?;
    validate_private_file(&file)?;
    Ok(file)
}

fn open_new_private_file(path: &Path) -> Result<File, GuestJournalErrorV1> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options.open(path).map_err(|_| GuestJournalErrorV1::Io)?;
    validate_private_file(&file)?;
    Ok(file)
}

fn open_read_private_file(path: &Path) -> Result<File, GuestJournalErrorV1> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options.open(path).map_err(|_| GuestJournalErrorV1::Io)?;
    validate_private_file(&file)?;
    Ok(file)
}

fn validate_private_file(file: &File) -> Result<(), GuestJournalErrorV1> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = file.metadata().map_err(|_| GuestJournalErrorV1::Io)?;
        if !metadata.file_type().is_file() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(GuestJournalErrorV1::OwnershipMismatch);
        }
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| GuestJournalErrorV1::Io)?;
    }
    Ok(())
}

pub(super) fn action_digest(
    action: &crate::computer_use::ComputerActionV1,
) -> Result<String, GuestJournalErrorV1> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(action).map_err(|_| GuestJournalErrorV1::Corrupt)?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
