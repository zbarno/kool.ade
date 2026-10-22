//! Private, operator-local metrics for remote workspace transfer and execution.
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

mod legacy;
mod query;
mod validation;
use legacy::{normalize_legacy_metrics, now_unix_ms};
pub use query::latest_for_task;
use validation::{validate, validate_id, validate_worker_id};

#[cfg(test)]
#[path = "remote_execution_telemetry/tests.rs"]
mod tests;

const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RemoteExecutionTelemetryV1 {
    pub schema_version: u8,
    pub execution_id: String,
    #[serde(default)]
    pub started_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub task_id: Option<String>,
    pub worker_id: String,
    #[serde(default)]
    pub workspace_upload_bytes: Option<u64>,
    #[serde(default)]
    pub workspace_upload_ms: Option<u64>,
    #[serde(default)]
    pub result_download_bytes: Option<u64>,
    #[serde(default)]
    pub result_download_ms: Option<u64>,
    #[serde(default)]
    pub worker_start_rpc_ms: Option<u64>,
    #[serde(default)]
    pub execution_wall_ms: Option<u64>,
    #[serde(default)]
    pub execution_cpu_millis: Option<u64>,
    #[serde(default)]
    pub execution_peak_memory_bytes: Option<u64>,
    #[serde(default)]
    pub image_digest: Option<String>,
    #[serde(default)]
    pub failure_code: Option<crate::execution::errors::ExecutionErrorKind>,
}

pub fn record_workspace_upload(
    execution_id: &str,
    task_id: &str,
    worker_id: &str,
    bytes: u64,
    ms: u64,
) -> anyhow::Result<()> {
    update(execution_id, task_id, worker_id, |record| {
        record.workspace_upload_bytes = Some(bytes);
        record.workspace_upload_ms = Some(ms);
    })
}

pub fn record_result_download(
    execution_id: &str,
    task_id: &str,
    worker_id: &str,
    bytes: u64,
    ms: u64,
) -> anyhow::Result<()> {
    update(execution_id, task_id, worker_id, |record| {
        record.result_download_bytes = Some(bytes);
        record.result_download_ms = Some(ms);
    })
}

pub fn record_execution_started(
    execution_id: &str,
    task_id: &str,
    worker_id: &str,
    image_digest: &str,
    worker_start_rpc_ms: u64,
) -> anyhow::Result<()> {
    update(execution_id, task_id, worker_id, |record| {
        record.image_digest = Some(image_digest.to_owned());
        record.worker_start_rpc_ms = Some(worker_start_rpc_ms);
    })
}

pub fn record_execution_result(
    execution_id: &str,
    task_id: &str,
    worker_id: &str,
    wall_ms: Option<u64>,
    cpu_millis: u64,
    peak_memory_bytes: u64,
    failure_code: Option<crate::execution::errors::ExecutionErrorKind>,
) -> anyhow::Result<()> {
    update(execution_id, task_id, worker_id, |record| {
        if wall_ms.is_some() {
            record.execution_wall_ms = wall_ms;
        }
        record.execution_cpu_millis = Some(cpu_millis);
        record.execution_peak_memory_bytes = Some(peak_memory_bytes);
        record.failure_code = failure_code;
    })
}

pub fn load(execution_id: &str) -> anyhow::Result<Option<RemoteExecutionTelemetryV1>> {
    load_at(&crate::persistence::state_root(), execution_id)
}

fn load_at(root: &Path, execution_id: &str) -> anyhow::Result<Option<RemoteExecutionTelemetryV1>> {
    validate_id(execution_id)?;
    let path = record_path(root, execution_id);
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let mut record: RemoteExecutionTelemetryV1 = serde_json::from_slice(&read_private(&path)?)?;
    normalize_legacy_metrics(&mut record);
    validate(&record)?;
    anyhow::ensure!(
        record.execution_id == execution_id,
        "remote execution telemetry identity mismatch"
    );
    Ok(Some(record))
}

fn update(
    execution_id: &str,
    task_id: &str,
    worker_id: &str,
    change: impl FnOnce(&mut RemoteExecutionTelemetryV1),
) -> anyhow::Result<()> {
    update_at(
        &crate::persistence::state_root(),
        execution_id,
        task_id,
        worker_id,
        change,
    )
}

fn update_at(
    root: &Path,
    execution_id: &str,
    task_id: &str,
    worker_id: &str,
    change: impl FnOnce(&mut RemoteExecutionTelemetryV1),
) -> anyhow::Result<()> {
    validate_id(execution_id)?;
    validate_id(task_id)?;
    validate_worker_id(worker_id)?;
    let directory = root.join("remote-execution-telemetry");
    fs::create_dir_all(&directory)?;
    ensure_private_directory(&directory)?;
    let lock = telemetry_lock(&directory)?;
    fs2::FileExt::lock_exclusive(&lock)?;
    let path = record_path(root, execution_id);
    let mut record = if path.exists() {
        let previous_modified_ms = fs::metadata(&path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64);
        let mut record: RemoteExecutionTelemetryV1 = serde_json::from_slice(&read_private(&path)?)?;
        normalize_legacy_metrics(&mut record);
        if record.started_at_unix_ms.is_none() {
            record.started_at_unix_ms = previous_modified_ms;
        }
        validate(&record)?;
        anyhow::ensure!(
            record.worker_id == worker_id,
            "worker identity changed for execution"
        );
        anyhow::ensure!(
            record
                .task_id
                .as_deref()
                .is_none_or(|existing| existing == task_id),
            "task identity changed for execution"
        );
        record
    } else {
        RemoteExecutionTelemetryV1 {
            schema_version: 5,
            execution_id: execution_id.to_owned(),
            started_at_unix_ms: Some(now_unix_ms()),
            task_id: Some(task_id.to_owned()),
            worker_id: worker_id.to_owned(),
            workspace_upload_bytes: None,
            workspace_upload_ms: None,
            result_download_bytes: None,
            result_download_ms: None,
            worker_start_rpc_ms: None,
            execution_wall_ms: None,
            execution_cpu_millis: None,
            execution_peak_memory_bytes: None,
            image_digest: None,
            failure_code: None,
        }
    };
    record.task_id = Some(task_id.to_owned());
    change(&mut record);
    record.schema_version = 5;
    validate(&record)?;
    let bytes = serde_json::to_vec(&record)?;
    let temp = directory.join(format!(".{execution_id}-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    use std::io::Write;
    options.open(&temp)?.write_all(&bytes)?;
    fs::rename(temp, path)?;
    Ok(())
}

fn telemetry_lock(directory: &Path) -> anyhow::Result<fs::File> {
    let path = directory.join(".lock");
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    anyhow::ensure!(
        file.metadata()?.is_file(),
        "remote execution telemetry lock is invalid"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn record_path(root: &Path, id: &str) -> std::path::PathBuf {
    root.join("remote-execution-telemetry")
        .join(format!("{id}.json"))
}

fn read_private(path: &Path) -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= 64 * 1024,
        "remote execution telemetry file is invalid"
    );
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut bytes = Vec::new();
    options
        .open(path)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= 64 * 1024,
        "remote execution telemetry file exceeds its limit"
    );
    Ok(bytes)
}

fn ensure_private_directory(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "remote execution telemetry directory is invalid"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
