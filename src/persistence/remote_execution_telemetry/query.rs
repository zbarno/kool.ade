use super::RemoteExecutionTelemetryV1;
use std::path::Path;

pub fn latest_for_task(task_id: &str) -> anyhow::Result<Option<RemoteExecutionTelemetryV1>> {
    latest_for_task_at(&crate::persistence::state_root(), task_id)
}

pub(super) fn latest_for_task_at(
    root: &Path,
    task_id: &str,
) -> anyhow::Result<Option<RemoteExecutionTelemetryV1>> {
    super::validate_id(task_id)?;
    let directory = root.join("remote-execution-telemetry");
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut latest: Option<(u128, RemoteExecutionTelemetryV1)> = None;
    for entry in entries {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        let path = entry.path();
        let Some(execution_id) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if uuid::Uuid::parse_str(execution_id).is_err() {
            continue;
        }
        let Some(record) = super::load_at(root, execution_id)? else {
            continue;
        };
        if record.task_id.as_deref() != Some(task_id) {
            continue;
        }
        let order = record.started_at_unix_ms.map_or_else(
            || {
                entry
                    .metadata()
                    .ok()
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |duration| duration.as_millis())
            },
            u128::from,
        );
        if latest.as_ref().is_none_or(|(time, _)| order > *time) {
            latest = Some((order, record));
        }
    }
    Ok(latest.map(|(_, record)| record))
}
