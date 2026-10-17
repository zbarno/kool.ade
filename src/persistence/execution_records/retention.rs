use std::{fs, path::Path};

use fs2::FileExt;

use super::{ExecutionCleanupStateV1, ExecutionRecordStateV1, record::is_canonical_uuid};

const MAX_COMPLETED_RECORDS: usize = 500;
const MAX_COMPLETED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

struct CompletedRecord {
    task_id: String,
    execution_id: String,
    owner_instance_id: String,
    updated_at_unix_ms: u64,
    bytes: u64,
}

pub(super) fn with_retention_lock<T>(
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let state_root = crate::persistence::state_root();
    match fs::symlink_metadata(&state_root) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err("application state root is not a real directory".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&state_root).map_err(|error| error.to_string())?;
            let metadata = fs::symlink_metadata(&state_root).map_err(|error| error.to_string())?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err("application state root is not a real directory".into());
            }
        }
        Err(error) => return Err(error.to_string()),
    }
    let lock_path = state_root.join("execution-retention.lock");
    with_retention_lock_path(&lock_path, work)
}

pub(super) fn with_retention_lock_path<T>(
    lock_path: &Path,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let lock = open_regular_file(lock_path, true)?
        .ok_or_else(|| "execution retention lock is unavailable".to_owned())?;
    lock.lock_exclusive().map_err(|error| error.to_string())?;
    work()
}

fn open_regular_file(path: &Path, create: bool) -> Result<Option<fs::File>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err("execution retention lock is not a regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let mut options = fs::OpenOptions::new();
    options
        .create(create)
        .truncate(false)
        .read(true)
        .write(true);
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
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("execution retention lock is not a regular file".into());
    }
    Ok(Some(file))
}

pub(super) fn prune_completed_locked(
    owner_instance_id: &str,
    protected_task: &str,
    protected_execution: &str,
) -> Result<(), String> {
    prune_completed_with_limits(
        &crate::persistence::state_root().join("executions"),
        MAX_COMPLETED_RECORDS,
        MAX_COMPLETED_BYTES,
        owner_instance_id,
        Some((protected_task, protected_execution)),
    )
}

pub(super) fn prune_completed_with_limits(
    root: &Path,
    max_records: usize,
    max_bytes: u64,
    owner_instance_id: &str,
    protected: Option<(&str, &str)>,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("execution retention root is not a real directory".into());
    }
    let mut completed = Vec::new();
    for task_entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let task_entry = task_entry.map_err(|error| error.to_string())?;
        let task_id = task_entry
            .file_name()
            .into_string()
            .map_err(|_| "execution task ID is not valid UTF-8".to_owned())?;
        if !is_canonical_uuid(&task_id) {
            if matches!(
                task_id.as_str(),
                "owner-instance-id" | "owner-instance.lock"
            ) {
                continue;
            }
            return Err("execution retention found an unknown root entry".into());
        }
        let task_dir = task_entry.path();
        let task_metadata = fs::symlink_metadata(&task_dir).map_err(|error| error.to_string())?;
        if !task_metadata.is_dir() || task_metadata.file_type().is_symlink() {
            return Err("execution retention task path is not a real directory".into());
        }
        for entry in fs::read_dir(&task_dir).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "execution retention filename is invalid".to_owned())?;
            if name == "task.lock" {
                let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err("execution retention task lock is not a regular file".into());
                }
                continue;
            }
            if let Some(execution_id) = name.strip_suffix(".events") {
                if !is_canonical_uuid(execution_id) {
                    return Err("execution retention journal identity is invalid".into());
                }
                let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err("execution retention journal is not a real directory".into());
                }
                let record_path = task_dir.join(format!("{execution_id}.json"));
                let record_metadata = fs::symlink_metadata(record_path)
                    .map_err(|_| "execution retention journal has no matching record".to_owned())?;
                if !record_metadata.is_file() || record_metadata.file_type().is_symlink() {
                    return Err("execution retention journal has no matching record".into());
                }
                continue;
            }
            let Some(execution_id) = name.strip_suffix(".json") else {
                return Err("execution retention found an unknown task entry".into());
            };
            if !is_canonical_uuid(execution_id) {
                return Err("execution retention record identity is invalid".into());
            }
            let record = super::load(&task_id, execution_id)?
                .ok_or_else(|| "execution retention record disappeared".to_owned())?;
            if record.owner_instance_id != owner_instance_id || !is_terminal_and_clean(&record) {
                continue;
            }
            let record_metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if !record_metadata.is_file() || record_metadata.file_type().is_symlink() {
                return Err("execution retention record is not a regular file".into());
            }
            super::journal::validate_journal(&task_id, execution_id)?;
            let journal_path = task_dir.join(format!("{execution_id}.events"));
            let journal_bytes = journal_size(&journal_path)?;
            completed.push(CompletedRecord {
                task_id: task_id.clone(),
                execution_id: execution_id.to_owned(),
                owner_instance_id: record.owner_instance_id,
                updated_at_unix_ms: record.updated_at_unix_ms,
                bytes: record_metadata.len().saturating_add(journal_bytes),
            });
        }
    }
    completed.sort_by(|left, right| {
        (left.updated_at_unix_ms, &left.task_id, &left.execution_id).cmp(&(
            right.updated_at_unix_ms,
            &right.task_id,
            &right.execution_id,
        ))
    });
    let mut total_bytes = completed
        .iter()
        .fold(0_u64, |sum, record| sum.saturating_add(record.bytes));
    let mut retained_count = completed.len();
    while retained_count > max_records || total_bytes > max_bytes {
        let Some(index) = completed.iter().position(|record| {
            protected != Some((record.task_id.as_str(), record.execution_id.as_str()))
        }) else {
            return Err("execution completed-history quota cannot be met safely".into());
        };
        let record = completed.remove(index);
        let Some(_task_lock) = try_candidate_task_lock(&record.task_id)? else {
            continue;
        };
        let Some(current) = super::load(&record.task_id, &record.execution_id)? else {
            retained_count = retained_count.saturating_sub(1);
            total_bytes = total_bytes.saturating_sub(record.bytes);
            continue;
        };
        if !is_terminal_and_clean(&current) || current.owner_instance_id != owner_instance_id {
            retained_count = retained_count.saturating_sub(1);
            total_bytes = total_bytes.saturating_sub(record.bytes);
            continue;
        }
        if current.owner_instance_id != record.owner_instance_id
            || current.updated_at_unix_ms != record.updated_at_unix_ms
        {
            return Err("execution record changed during retention".into());
        }
        super::journal::validate_journal(&record.task_id, &record.execution_id)?;
        remove_completed(root, &record)?;
        retained_count = retained_count.saturating_sub(1);
        total_bytes = total_bytes.saturating_sub(record.bytes);
    }
    Ok(())
}

fn is_terminal_and_clean(record: &super::ExecutionRecordV1) -> bool {
    matches!(
        record.state,
        ExecutionRecordStateV1::Succeeded
            | ExecutionRecordStateV1::Failed
            | ExecutionRecordStateV1::Cancelled
            | ExecutionRecordStateV1::Interrupted
    ) && record.cleanup == ExecutionCleanupStateV1::Complete
}

fn try_candidate_task_lock(task_id: &str) -> Result<Option<fs::File>, String> {
    if !is_canonical_uuid(task_id) {
        return Err("execution retention task identity is invalid".into());
    }
    let task_lock = crate::persistence::state_root()
        .join("executions")
        .join(task_id)
        .join("task.lock");
    let Some(file) = open_regular_file(&task_lock, true)? else {
        return Ok(None);
    };
    match FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn journal_size(path: &Path) -> Result<u64, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("execution retention journal is not a real directory".into());
    }
    let mut bytes = 0_u64;
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("execution retention journal entry is not a regular file".into());
        }
        bytes = bytes.saturating_add(metadata.len());
    }
    Ok(bytes)
}

fn remove_completed(root: &Path, record: &CompletedRecord) -> Result<(), String> {
    let task_dir = root.join(&record.task_id);
    let journal = task_dir.join(format!("{}.events", record.execution_id));
    if journal.exists() {
        fs::remove_dir_all(journal).map_err(|error| error.to_string())?;
    }
    fs::remove_file(task_dir.join(format!("{}.json", record.execution_id)))
        .map_err(|error| error.to_string())
}
