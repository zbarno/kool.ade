use std::{fs, path::PathBuf};

use super::{ExecutionRecordV1, record::is_canonical_uuid};

const MAX_RECORD_BYTES: u64 = 1024 * 1024;

fn record_path(record: &ExecutionRecordV1) -> PathBuf {
    crate::persistence::state_root()
        .join("executions")
        .join(&record.task_id)
        .join(format!("{}.json", record.execution_id))
}

/// Atomically save a validated execution record under application-owned state.
pub fn save(record: &ExecutionRecordV1) -> Result<(), String> {
    record.validate()?;
    let bytes = serde_json::to_vec(record).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err("execution record exceeds its size limit".into());
    }
    let owner = is_terminal_and_clean(record)
        .then(super::owner::owner_instance_id)
        .transpose()?;
    super::retention::with_retention_lock(|| {
        persist_record(record, &bytes)?;
        if let Some(owner) = owner {
            super::retention::prune_completed_locked(
                &owner,
                &record.task_id,
                &record.execution_id,
            )?;
        }
        Ok(())
    })?;
    Ok(())
}

fn persist_record(record: &ExecutionRecordV1, bytes: &[u8]) -> Result<(), String> {
    crate::artifacts::atomic_write_bytes(&record_path(record), bytes)
        .map_err(|error| error.to_string())
}

fn is_terminal_and_clean(record: &ExecutionRecordV1) -> bool {
    matches!(
        record.state,
        super::ExecutionRecordStateV1::Succeeded
            | super::ExecutionRecordStateV1::Failed
            | super::ExecutionRecordStateV1::Cancelled
            | super::ExecutionRecordStateV1::Interrupted
    ) && record.cleanup == super::ExecutionCleanupStateV1::Complete
}

/// Load a task's execution record. Malformed or unsupported records fail closed.
pub fn load(task_id: &str, execution_id: &str) -> Result<Option<ExecutionRecordV1>, String> {
    if !is_canonical_uuid(task_id) || !is_canonical_uuid(execution_id) {
        return Err("execution record lookup identity is invalid".into());
    }
    let path = crate::persistence::state_root()
        .join("executions")
        .join(task_id)
        .join(format!("{execution_id}.json"));
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_RECORD_BYTES {
        return Err("execution record exceeds its size limit".into());
    }
    let record: ExecutionRecordV1 =
        serde_json::from_reader(file).map_err(|error| error.to_string())?;
    record.validate()?;
    if record.task_id != task_id || record.execution_id != execution_id {
        return Err("execution record path does not match its identity".into());
    }
    Ok(Some(record))
}

/// Load the most recently updated durable execution record for a task.
pub fn latest_for_task(task_id: &str) -> Result<Option<ExecutionRecordV1>, String> {
    if !is_canonical_uuid(task_id) {
        return Err("execution record lookup identity is invalid".into());
    }
    let directory = crate::persistence::state_root()
        .join("executions")
        .join(task_id);
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut latest: Option<ExecutionRecordV1> = None;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Some(execution_id) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        if !is_canonical_uuid(execution_id) {
            continue;
        }
        let Some(record) = load(task_id, execution_id)? else {
            continue;
        };
        if latest.as_ref().is_none_or(|current| {
            (record.updated_at_unix_ms, record.started_at_unix_ms)
                > (current.updated_at_unix_ms, current.started_at_unix_ms)
        }) {
            latest = Some(record);
        }
    }
    Ok(latest)
}
