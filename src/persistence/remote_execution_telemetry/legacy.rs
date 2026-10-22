use super::RemoteExecutionTelemetryV1;

pub(super) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(super) fn normalize_legacy_metrics(record: &mut RemoteExecutionTelemetryV1) {
    // Older writers initialized unobserved phase counters to zero. New records
    // carry a start timestamp, so its absence identifies those ambiguous values.
    if record.started_at_unix_ms.is_none() {
        if record.workspace_upload_bytes == Some(0) && record.workspace_upload_ms == Some(0) {
            record.workspace_upload_bytes = None;
            record.workspace_upload_ms = None;
        }
        if record.result_download_bytes == Some(0) && record.result_download_ms == Some(0) {
            record.result_download_bytes = None;
            record.result_download_ms = None;
        }
        if record.execution_wall_ms.is_none() {
            if record.execution_cpu_millis == Some(0) {
                record.execution_cpu_millis = None;
            }
            if record.execution_peak_memory_bytes == Some(0) {
                record.execution_peak_memory_bytes = None;
            }
        }
    }
}
