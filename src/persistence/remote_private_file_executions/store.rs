use super::{
    MAX_ACTIVE_EXECUTIONS_PER_WORKER, MAX_RECORD_BYTES, RemotePrivateFileExecutionGrantV1,
    validate_uuid,
};
use paths::{ensure_private_file, is_atomic_temp_name, lock_directory};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read},
    path::Path,
    sync::Mutex,
};

static LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn save(root: &Path, record: &RemotePrivateFileExecutionGrantV1) -> anyhow::Result<()> {
    record.validate()?;
    let _guard = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("private file execution lock is unavailable"))?;
    let directory = worker_directory(root, &record.worker_id)?;
    let _file_lock = lock_directory(&directory)?;
    save_at(&directory, record)
}

pub(crate) fn load_for_worker(
    root: &Path,
    worker_id: &str,
) -> anyhow::Result<Vec<RemotePrivateFileExecutionGrantV1>> {
    validate_uuid(worker_id)?;
    let _guard = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("private file execution lock is unavailable"))?;
    let directory = worker_directory(root, worker_id)?;
    let _file_lock = lock_directory(&directory)?;
    load_for_worker_at(&directory, worker_id)
}

pub(crate) fn mark_stage_complete(
    root: &Path,
    worker_id: &str,
    execution_id: &str,
) -> anyhow::Result<()> {
    update_record(root, worker_id, execution_id, |record| {
        record.stage_complete = true;
        Ok(())
    })
}

pub(crate) fn bind_start_request(
    root: &Path,
    worker_id: &str,
    execution_id: &str,
    sequence: u64,
) -> anyhow::Result<()> {
    anyhow::ensure!(sequence > 0);
    update_record(root, worker_id, execution_id, |record| {
        anyhow::ensure!(record.stage_complete);
        match record.start_request_sequence {
            Some(existing) => anyhow::ensure!(existing == sequence),
            None => record.start_request_sequence = Some(sequence),
        }
        Ok(())
    })
}

fn update_record(
    root: &Path,
    worker_id: &str,
    execution_id: &str,
    update: impl FnOnce(&mut RemotePrivateFileExecutionGrantV1) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    validate_uuid(worker_id)?;
    validate_uuid(execution_id)?;
    let _guard = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("private file execution lock is unavailable"))?;
    let directory = worker_directory(root, worker_id)?;
    let _file_lock = lock_directory(&directory)?;
    let path = record_path(&directory, execution_id);
    let mut record = read_record(&path)?
        .ok_or_else(|| anyhow::anyhow!("private file execution link is unavailable"))?;
    anyhow::ensure!(record.worker_id == worker_id && record.execution_id == execution_id);
    record.validate()?;
    update(&mut record)?;
    record.validate()?;
    let bytes = serde_json::to_vec_pretty(&record)?;
    crate::artifacts::atomic_write_bytes(&path, &bytes)?;
    ensure_private_file(&path)
}

pub(crate) fn remove(root: &Path, worker_id: &str, execution_id: &str) -> anyhow::Result<()> {
    validate_uuid(worker_id)?;
    validate_uuid(execution_id)?;
    let _guard = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("private file execution lock is unavailable"))?;
    let directory = worker_directory(root, worker_id)?;
    let _file_lock = lock_directory(&directory)?;
    let path = record_path(&directory, execution_id);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "private file execution record is unsafe"
            );
            fs::remove_file(path)?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn save_at(directory: &Path, record: &RemotePrivateFileExecutionGrantV1) -> anyhow::Result<()> {
    let path = record_path(directory, &record.execution_id);
    if let Some(existing) = read_record(&path)? {
        anyhow::ensure!(
            existing == *record,
            "private file execution ID conflicts with a prior grant link"
        );
        return Ok(());
    }
    anyhow::ensure!(
        load_for_worker_at(directory, &record.worker_id)?.len() < MAX_ACTIVE_EXECUTIONS_PER_WORKER,
        "private file execution grant store is full"
    );
    let bytes = serde_json::to_vec_pretty(record)?;
    crate::artifacts::atomic_write_bytes(&path, &bytes)?;
    ensure_private_file(&path)?;
    Ok(())
}

fn load_for_worker_at(
    directory: &Path,
    worker_id: &str,
) -> anyhow::Result<Vec<RemotePrivateFileExecutionGrantV1>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut records = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("private file execution filename is invalid"))?;
        if name == ".lock" {
            ensure_private_file(&path)?;
            continue;
        }
        if is_atomic_temp_name(name) {
            let metadata = fs::symlink_metadata(&path)?;
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "private file execution temporary file is unsafe"
            );
            fs::remove_file(path)?;
            continue;
        }
        let record = read_record(&path)?
            .ok_or_else(|| anyhow::anyhow!("private file execution record is missing"))?;
        record.validate()?;
        anyhow::ensure!(
            record.worker_id == worker_id && path == record_path(directory, &record.execution_id),
            "private file execution record binding is invalid"
        );
        records.push(record);
        anyhow::ensure!(
            records.len() <= MAX_ACTIVE_EXECUTIONS_PER_WORKER,
            "private file execution grant store is oversized"
        );
    }
    records.sort_by(|left, right| left.execution_id.cmp(&right.execution_id));
    Ok(records)
}

fn read_record(path: &Path) -> anyhow::Result<Option<RemotePrivateFileExecutionGrantV1>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() <= MAX_RECORD_BYTES,
        "private file execution record is unsafe"
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    options
        .open(path)?
        .take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= MAX_RECORD_BYTES);
    Ok(Some(serde_json::from_slice(&bytes)?))
}

mod paths;
pub(super) use paths::{record_path, worker_directory};
