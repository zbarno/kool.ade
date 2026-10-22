use super::{RemoteExecutionTelemetryV1, update};
use crate::execution::contract::ExecutionResultV1;

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
    result: &ExecutionResultV1,
) -> anyhow::Result<()> {
    update(
        execution_id,
        task_id,
        worker_id,
        |record: &mut RemoteExecutionTelemetryV1| {
            if wall_ms.is_some() {
                record.execution_wall_ms = wall_ms;
            }
            record.execution_cpu_millis = Some(result.metrics.cpu_millis);
            record.execution_peak_memory_bytes = Some(result.metrics.peak_memory_bytes);
            record.failure_code = result.failure.as_ref().map(|failure| failure.kind);
            if !result.phase_timings.is_empty() {
                record.execution_phase_timings = Some(result.phase_timings.clone());
            }
        },
    )
}
