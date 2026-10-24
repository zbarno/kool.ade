use std::fs;

use super::{
    ExecutionCleanupStateV1, ExecutionRecordStateV1, ExecutionRecordV1, record::is_canonical_uuid,
};

const OWNER_FILE: &str = "owner-instance-id";
const MAX_RECORD_COUNT: usize = 10_000;

/// Return the stable local owner identity used to reconcile this installation's containers.
pub fn owner_instance_id() -> Result<String, String> {
    owner_instance_id_at(&crate::persistence::state_root())
}

pub(crate) fn owner_instance_id_at(state_root: &std::path::Path) -> Result<String, String> {
    use std::io::Read;

    let root = state_root.join("executions");
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let metadata = fs::symlink_metadata(&root).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("execution state directory is not a real directory".into());
    }
    let lock_path = root.join("owner-instance.lock");
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|error| error.to_string())?;
    fs2::FileExt::lock_exclusive(&lock).map_err(|error| error.to_string())?;
    let path = root.join(OWNER_FILE);
    if let Ok(metadata) = fs::symlink_metadata(&path)
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        let _ = fs2::FileExt::unlock(&lock);
        return Err("execution owner identity is not a regular file".into());
    }
    let result = match fs::File::open(&path) {
        Ok(file) => {
            let mut value = String::new();
            file.take(128)
                .read_to_string(&mut value)
                .map_err(|error| error.to_string())?;
            let value = value.trim();
            if is_canonical_uuid(value) {
                Ok(value.to_owned())
            } else {
                Err("execution owner identity file is invalid".into())
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let value = uuid::Uuid::new_v4().hyphenated().to_string();
            crate::artifacts::atomic_write_bytes(&path, value.as_bytes())
                .map_err(|error| error.to_string())?;
            Ok(value)
        }
        Err(error) => Err(error.to_string()),
    };
    let _ = fs2::FileExt::unlock(&lock);
    result
}

/// Acquire the per-task process lock; `None` means another app instance owns it.
pub fn try_task_lock(task_id: &str) -> Result<Option<fs::File>, String> {
    use std::fs::OpenOptions;

    if !is_canonical_uuid(task_id) {
        return Err("execution task lock identity is invalid".into());
    }
    let directory = crate::persistence::state_root()
        .join("executions")
        .join(task_id);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let metadata = fs::symlink_metadata(&directory).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("execution task state path is not a real directory".into());
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("task.lock"))
        .map_err(|error| error.to_string())?;
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// Enumerate records that may still own resources for one application instance.
/// A malformed record fails the scan so recovery never silently skips unknown state.
pub fn unfinished_for_owner(owner_instance_id: &str) -> Result<Vec<ExecutionRecordV1>, String> {
    unfinished_for_owner_at(&crate::persistence::state_root(), owner_instance_id)
}

pub(crate) fn unfinished_for_owner_at(
    state_root: &std::path::Path,
    owner_instance_id: &str,
) -> Result<Vec<ExecutionRecordV1>, String> {
    if !is_canonical_uuid(owner_instance_id) {
        return Err("execution owner identity is invalid".into());
    }
    let root = state_root.join("executions");
    let metadata = match fs::symlink_metadata(&root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("execution record directory is not a real directory".into());
    }
    let lock_path = state_root.join("execution-retention.lock");
    super::retention::with_retention_lock_path(&lock_path, || {
        scan_unfinished_for_owner_at(state_root, owner_instance_id)
    })
}

fn scan_unfinished_for_owner_at(
    state_root: &std::path::Path,
    owner_instance_id: &str,
) -> Result<Vec<ExecutionRecordV1>, String> {
    let root = state_root.join("executions");
    let metadata = match fs::symlink_metadata(&root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("execution record directory is not a real directory".into());
    }
    let mut records = Vec::new();
    for task_entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let task_entry = task_entry.map_err(|error| error.to_string())?;
        let task_path = task_entry.path();
        let task_metadata = fs::symlink_metadata(&task_path).map_err(|error| error.to_string())?;
        if task_entry.file_name() == OWNER_FILE || task_entry.file_name() == "owner-instance.lock" {
            if !task_metadata.is_file() || task_metadata.file_type().is_symlink() {
                return Err("execution owner state is not a regular file".into());
            }
            continue;
        }
        if !task_metadata.is_dir() || task_metadata.file_type().is_symlink() {
            return Err("execution task record path is not a real directory".into());
        }
        let task_id = task_entry
            .file_name()
            .into_string()
            .map_err(|_| "execution task record ID is not valid UTF-8".to_owned())?;
        if !is_canonical_uuid(&task_id) {
            return Err("execution task record directory has an invalid ID".into());
        }
        for record_entry in fs::read_dir(&task_path).map_err(|error| error.to_string())? {
            let record_entry = record_entry.map_err(|error| error.to_string())?;
            let record_path = record_entry.path();
            let file_metadata =
                fs::symlink_metadata(&record_path).map_err(|error| error.to_string())?;
            let file_name = record_entry
                .file_name()
                .into_string()
                .map_err(|_| "execution state filename is not valid UTF-8".to_owned())?;
            if file_name == "task.lock" {
                if !file_metadata.is_file() || file_metadata.file_type().is_symlink() {
                    return Err("execution task lock is not a regular file".into());
                }
                continue;
            }
            if let Some(execution_id) = file_name.strip_suffix(".events") {
                if !is_canonical_uuid(execution_id)
                    || !file_metadata.is_dir()
                    || file_metadata.file_type().is_symlink()
                {
                    return Err("execution journal path is invalid".into());
                }
                let record_file = task_path.join(format!("{execution_id}.json"));
                let record_metadata = fs::symlink_metadata(&record_file)
                    .map_err(|_| "execution journal has no matching record".to_owned())?;
                if !record_metadata.is_file() || record_metadata.file_type().is_symlink() {
                    return Err("execution journal has no matching record".into());
                }
                super::journal::validate_journal(&task_id, execution_id)?;
                continue;
            }
            if !file_metadata.is_file() || file_metadata.file_type().is_symlink() {
                return Err("execution record is not a regular file".into());
            }
            let Some(execution_id) = record_path.file_stem().and_then(|value| value.to_str())
            else {
                return Err("execution record filename is invalid".into());
            };
            if record_path.extension().and_then(|value| value.to_str()) != Some("json")
                || !is_canonical_uuid(execution_id)
            {
                return Err("execution record filename is invalid".into());
            }
            let Some(record) = super::store::load_at(state_root, &task_id, execution_id)? else {
                return Err("execution record disappeared during recovery scan".into());
            };
            let terminal_and_clean = matches!(
                record.state,
                ExecutionRecordStateV1::Succeeded
                    | ExecutionRecordStateV1::Failed
                    | ExecutionRecordStateV1::Cancelled
                    | ExecutionRecordStateV1::Interrupted
            ) && record.cleanup == ExecutionCleanupStateV1::Complete;
            if record.owner_instance_id == owner_instance_id && !terminal_and_clean {
                records.push(record);
                if records.len() > MAX_RECORD_COUNT {
                    return Err("execution recovery record count exceeds its limit".into());
                }
            }
        }
    }
    records.sort_by_key(|record| (record.started_at_unix_ms, record.execution_id.clone()));
    Ok(records)
}
