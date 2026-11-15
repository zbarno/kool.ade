use super::{ClaimError, ClaimExecutionAttempt, ClaimExecutionAttemptStatus, ClaimLease};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

impl ClaimLease {
    pub(crate) fn acquire(
        state_root: &Path,
        project_uid: uuid::Uuid,
        task_uid: &str,
    ) -> Result<Self, ClaimError> {
        if task_uid.trim().is_empty() {
            return Err(ClaimError::CoordinationRejected(
                "Task identity is empty".into(),
            ));
        }
        let directory = ensure_private_directory(state_root)?;
        let lock_path = lease_path(&directory, project_uid, task_uid);
        let lock = open_lock_file(&lock_path, true)?;
        match lock.try_lock_exclusive() {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(ClaimError::AlreadyClaimed);
            }
            Err(error) => return Err(ClaimError::LocalStorage(error.to_string())),
        }
        let token = uuid::Uuid::new_v4().hyphenated().to_string();
        write_token(&lock, &token)?;
        Ok(Self {
            lock,
            lock_path,
            token,
            execution_attempt: None,
        })
    }

    pub(crate) fn is_held_by_another_process(
        state_root: &Path,
        project_uid: uuid::Uuid,
        task_uid: &str,
    ) -> Result<bool, ClaimError> {
        if task_uid.trim().is_empty() {
            return Err(ClaimError::CoordinationRejected(
                "Task identity is empty".into(),
            ));
        }
        if !state_root.is_absolute() {
            return Err(ClaimError::LocalStorage(
                "KOOLADE_HOME must resolve to an absolute path".into(),
            ));
        }
        let root_metadata = match fs::symlink_metadata(state_root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(local_storage(error)),
        };
        if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
            return Err(ClaimError::LocalStorage(
                "KOOLADE_HOME is not a trusted directory".into(),
            ));
        }
        let directory = state_root.join("task-fences");
        let metadata = match fs::symlink_metadata(&directory) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(local_storage(error)),
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(ClaimError::LocalStorage(
                "Task fence directory is not a trusted directory".into(),
            ));
        }
        validate_private_directory(&metadata)?;
        let path = lease_path(&directory, project_uid, task_uid);
        match fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(local_storage(error)),
        };
        let lock = open_lock_file(&path, false)?;
        match lock.try_lock_exclusive() {
            Ok(()) => {
                FileExt::unlock(&lock).map_err(local_storage)?;
                Ok(false)
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(true),
            Err(error) => Err(local_storage(error)),
        }
    }

    pub(crate) fn verify(&self) -> Result<(), ClaimError> {
        let mut current = String::new();
        let mut file = open_lock_file(&self.lock_path, false)?;
        file.read_to_string(&mut current).map_err(local_storage)?;
        if current != self.token || !same_file(&self.lock, &file).map_err(local_storage)? {
            return Err(ClaimError::CoordinationRejected(
                "The local task fence was replaced or is no longer valid".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn begin_execution_attempt(
        &mut self,
        attempt: ClaimExecutionAttempt,
    ) -> Result<(), ClaimError> {
        if !uuid::Uuid::parse_str(&attempt.attempt_id)
            .is_ok_and(|id| id.hyphenated().to_string() == attempt.attempt_id)
            || attempt.started_at_utc <= 0
        {
            return Err(ClaimError::CoordinationRejected(
                "Local execution attempt metadata is invalid".into(),
            ));
        }
        self.verify()?;
        self.execution_attempt = Some(attempt);
        Ok(())
    }

    pub(crate) fn set_execution_attempt_status(
        &mut self,
        attempt_id: &str,
        status: ClaimExecutionAttemptStatus,
    ) -> Result<(), ClaimError> {
        self.verify()?;
        let Some(attempt) = self.execution_attempt.as_mut() else {
            return Err(ClaimError::CoordinationRejected(
                "No local execution attempt is active".into(),
            ));
        };
        if attempt.attempt_id != attempt_id {
            return Err(ClaimError::CoordinationRejected(
                "Local execution attempt was superseded".into(),
            ));
        }
        attempt.status = status;
        Ok(())
    }

    pub(crate) fn verify_execution_attempt(&self, attempt_id: &str) -> Result<(), ClaimError> {
        self.verify()?;
        if self.execution_attempt.as_ref().is_some_and(|attempt| {
            attempt.attempt_id == attempt_id
                && attempt.status != ClaimExecutionAttemptStatus::ConfirmedLost
        }) {
            Ok(())
        } else {
            Err(ClaimError::CoordinationRejected(
                "Local execution attempt is no longer current".into(),
            ))
        }
    }
}

fn lease_path(directory: &Path, project_uid: uuid::Uuid, task_uid: &str) -> PathBuf {
    let mut key = sha2::Sha256::new();
    use sha2::Digest;
    key.update(project_uid.as_bytes());
    key.update([0]);
    key.update(task_uid.as_bytes());
    directory.join(format!("{}.lock", hex(&key.finalize())))
}

fn ensure_private_directory(state_root: &Path) -> Result<PathBuf, ClaimError> {
    if !state_root.is_absolute() {
        return Err(ClaimError::LocalStorage(
            "KOOLADE_HOME must resolve to an absolute path".into(),
        ));
    }
    fs::create_dir_all(state_root).map_err(local_storage)?;
    let root_metadata = fs::symlink_metadata(state_root).map_err(local_storage)?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(ClaimError::LocalStorage(
            "KOOLADE_HOME is not a trusted directory".into(),
        ));
    }
    let directory = state_root.join("task-fences");
    match fs::create_dir(&directory) {
        Ok(()) => set_private_directory_permissions(&directory)?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(local_storage(error)),
    }
    let metadata = fs::symlink_metadata(&directory).map_err(local_storage)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ClaimError::LocalStorage(
            "Task fence directory is not a trusted directory".into(),
        ));
    }
    validate_private_directory(&metadata)?;
    Ok(directory)
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), ClaimError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(local_storage)
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), ClaimError> {
    Ok(())
}

#[cfg(unix)]
fn validate_private_directory(metadata: &fs::Metadata) -> Result<(), ClaimError> {
    use std::os::unix::fs::PermissionsExt;
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(ClaimError::LocalStorage(
            "Task fence directory permissions are not private".into(),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_private_directory(_metadata: &fs::Metadata) -> Result<(), ClaimError> {
    Ok(())
}

fn open_lock_file(path: &Path, create: bool) -> Result<File, ClaimError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(create)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(local_storage)?;
    let metadata = fs::symlink_metadata(path).map_err(local_storage)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ClaimError::LocalStorage(
            "Task fence path is not a regular file".into(),
        ));
    }
    Ok(file)
}

#[cfg(unix)]
fn same_file(left: &File, right: &File) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let left = left.metadata()?;
    let right = right.metadata()?;
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
}

#[cfg(windows)]
fn same_file(left: &File, right: &File) -> std::io::Result<bool> {
    let left = same_file::Handle::from_file(left.try_clone()?)?;
    let right = same_file::Handle::from_file(right.try_clone()?)?;
    Ok(left == right)
}

#[cfg(not(any(unix, windows)))]
fn same_file(_left: &File, _right: &File) -> std::io::Result<bool> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Task fence file identity is unsupported on this platform",
    ))
}

fn write_token(file: &File, token: &str) -> Result<(), ClaimError> {
    let mut file = file;
    file.set_len(0).map_err(local_storage)?;
    file.seek(SeekFrom::Start(0)).map_err(local_storage)?;
    file.write_all(token.as_bytes()).map_err(local_storage)?;
    file.sync_all().map_err(local_storage)
}

fn local_storage(error: std::io::Error) -> ClaimError {
    ClaimError::LocalStorage(error.to_string())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
