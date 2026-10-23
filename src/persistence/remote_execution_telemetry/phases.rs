use super::{RemoteExecutionTelemetryV1, update};
use crate::execution::contract::ExecutionPhaseTimingsV1;

pub fn record_execution_phase_timings(
    execution_id: &str,
    task_id: &str,
    worker_id: &str,
    phase_timings: &ExecutionPhaseTimingsV1,
) -> anyhow::Result<()> {
    update(
        execution_id,
        task_id,
        worker_id,
        |record: &mut RemoteExecutionTelemetryV1| {
            if !phase_timings.is_empty() {
                record.execution_phase_timings = Some(phase_timings.clone());
            }
        },
    )
}
