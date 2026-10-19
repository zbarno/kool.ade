use super::{GrantStoreV1, MAX_STORE_BYTES};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) fn path(root: &Path, project_id: &str) -> anyhow::Result<PathBuf> {
    crate::execution::contract::validate_identifier(project_id)?;
    Ok(root
        .join("remote-private-file-grants")
        .join(format!("{project_id}.json")))
}

pub(super) fn lock(path: &Path) -> anyhow::Result<File> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("private file grant store has no parent"))?;
    create_private_dir(parent)?;
    let lock_path = path.with_extension("lock");
    if let Ok(metadata) = fs::symlink_metadata(&lock_path) {
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "private file grant lock is not a regular file"
        );
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(&lock_path)?;
    ensure_private_file(&lock_path)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(std::fs::TryLockError::WouldBlock) => {
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "another process is updating private file grants"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

pub(super) fn load(project_id: &str, path: &Path) -> anyhow::Result<GrantStoreV1> {
    let state = match read(path) {
        Ok(bytes) => serde_json::from_slice::<GrantStoreV1>(&bytes)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => GrantStoreV1::empty(project_id),
        Err(error) => return Err(error.into()),
    };
    state.validate(project_id)?;
    Ok(state)
}

pub(super) fn write(path: &Path, state: &GrantStoreV1) -> anyhow::Result<()> {
    state.validate(&state.project_id)?;
    let bytes = serde_json::to_vec_pretty(state)?;
    anyhow::ensure!(bytes.len() as u64 <= MAX_STORE_BYTES);
    crate::artifacts::atomic_write_bytes(path, &bytes)?;
    ensure_private_file(path)?;
    Ok(())
}

fn read(path: &Path) -> io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "private file grant store is not a regular file",
        ));
    }
    if metadata.len() > MAX_STORE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "private file grant store is oversized",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut bytes = Vec::new();
    options
        .open(path)?
        .take(MAX_STORE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "private file grant store is oversized",
        ));
    }
    Ok(bytes)
}

fn create_private_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "private file grant directory is not trusted"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn ensure_private_file(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "private file grant state is not a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
