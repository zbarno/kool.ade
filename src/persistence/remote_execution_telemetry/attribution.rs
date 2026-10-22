use super::{RemoteExecutionTelemetryV1, update};

pub fn record_attribution(
    execution_id: &str,
    task_id: &str,
    worker_id: &str,
    harness: &str,
    requested_model: Option<&str>,
) -> anyhow::Result<()> {
    update(
        execution_id,
        task_id,
        worker_id,
        |record: &mut RemoteExecutionTelemetryV1| {
            record.harness = Some(harness.to_owned());
            record.requested_model = requested_model.map(str::to_owned);
        },
    )
}
