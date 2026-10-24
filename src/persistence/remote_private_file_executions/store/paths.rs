use super::super::validate_uuid;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const DIRECTORY: &str = "remote-private-file-executions";

pub(in crate::persistence::remote_private_file_executions) fn worker_directory(
    root: &Path,
    worker_id: &str,
) -> anyhow::Result<PathBuf> {
    validate_uuid(worker_id)?;
    let storage = root.join(DIRECTORY);
    fs::create_dir_all(&storage)?;
    let storage_metadata = fs::symlink_metadata(&storage)?;
    anyhow::ensure!(
        storage_metadata.is_dir() && !storage_metadata.file_type().is_symlink(),
        "private file execution storage is unsafe"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&storage, fs::Permissions::from_mode(0o700))?;
    }
    let directory = storage.join(worker_id);
    fs::create_dir_all(&directory)?;
    let metadata = fs::symlink_metadata(&directory)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "private file execution directory is unsafe"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    }
    Ok(directory)
}

pub(super) fn lock_directory(directory: &Path) -> anyhow::Result<File> {
    let path = directory.join(".lock");
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "private file execution lock is unsafe"
        );
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    ensure_private_file(&directory.join(".lock"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "another process is updating private file execution links"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

pub(in crate::persistence::remote_private_file_executions) fn record_path(
    directory: &Path,
    execution_id: &str,
) -> PathBuf {
    directory.join(format!("{execution_id}.json"))
}

pub(super) fn is_atomic_temp_name(name: &str) -> bool {
    let Some(value) = name.strip_prefix('.') else {
        return false;
    };
    let Some((execution_id, suffix)) = value.split_once(".json.") else {
        return false;
    };
    let Some(suffix) = suffix.strip_suffix(".koolade.tmp") else {
        return false;
    };
    let Some((pid, sequence)) = suffix.split_once('-') else {
        return false;
    };
    validate_uuid(execution_id).is_ok()
        && !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !sequence.is_empty()
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn ensure_private_file(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "private file execution state is not a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
