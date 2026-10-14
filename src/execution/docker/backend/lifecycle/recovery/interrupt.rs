use crate::execution::{
    contract::{
        EventPageRequestV1, ExecutionEventKindV1, ExecutionEventV1, ExecutionLifecycleV1,
        ExecutionResultV1, ResourceMetricsV1,
    },
    errors::{ExecutionError, ExecutionErrorKind},
};
use crate::persistence::execution_records::{
    self, ExecutionCleanupStateV1, ExecutionRecordStateV1, ExecutionRecordV1,
};

pub(in crate::execution::docker::backend::lifecycle) fn mark_interrupted(
    record: &mut ExecutionRecordV1,
    cleanup_complete: bool,
    message: &'static str,
) -> Result<(), ExecutionError> {
    let now = crate::execution::docker::backend::now_unix_ms().max(record.started_at_unix_ms);
    if !matches!(
        record.state,
        ExecutionRecordStateV1::Succeeded
            | ExecutionRecordStateV1::Failed
            | ExecutionRecordStateV1::Cancelled
            | ExecutionRecordStateV1::Interrupted
    ) {
        let failure = ExecutionError::new(ExecutionErrorKind::WorkerDisconnected, message, true);
        let cursor = execution_records::journal_cursor(&record.task_id, &record.execution_id)
            .map_err(|_| interrupted_error())?;
        let last_event = if cursor == 0 {
            None
        } else {
            execution_records::load_events(
                &record.task_id,
                &record.execution_id,
                EventPageRequestV1 {
                    after_sequence: cursor.saturating_sub(1),
                    max_events: 1,
                },
            )
            .map_err(|_| interrupted_error())?
            .into_iter()
            .last()
        };
        let mut sequence = cursor;
        if sequence == 0 {
            sequence = 1;
            append_lifecycle(record, sequence, now, ExecutionLifecycleV1::Queued)?;
        }
        append_lifecycle(
            record,
            sequence.saturating_add(1),
            last_event
                .map(|event| event.timestamp_unix_ms)
                .unwrap_or(now)
                .max(now),
            ExecutionLifecycleV1::Interrupted,
        )?;
        record.state = ExecutionRecordStateV1::Interrupted;
        record.ended_at_unix_ms = Some(now);
        record.result = Some(ExecutionResultV1 {
            schema_version: 1,
            execution_id: record.execution_id.clone(),
            exit_code: None,
            signal: None,
            timed_out: false,
            cancelled: false,
            metrics: ResourceMetricsV1 {
                cpu_millis: 0,
                peak_memory_bytes: 0,
                process_count: 0,
                disk_bytes: 0,
            },
            artifacts: Vec::new(),
            failure: Some(failure),
        });
        record.event_cursor = sequence.saturating_add(1);
    }
    record.updated_at_unix_ms = now.max(record.updated_at_unix_ms);
    if matches!(
        record.state,
        ExecutionRecordStateV1::Succeeded
            | ExecutionRecordStateV1::Failed
            | ExecutionRecordStateV1::Cancelled
            | ExecutionRecordStateV1::Interrupted
    ) {
        record.ended_at_unix_ms = Some(
            record
                .ended_at_unix_ms
                .unwrap_or(record.updated_at_unix_ms)
                .max(record.updated_at_unix_ms),
        );
    }
    record.cleanup = if cleanup_complete {
        ExecutionCleanupStateV1::Complete
    } else {
        ExecutionCleanupStateV1::Pending
    };
    execution_records::save(record).map_err(|_| interrupted_error())
}

fn append_lifecycle(
    record: &ExecutionRecordV1,
    sequence: u64,
    timestamp_unix_ms: u64,
    state: ExecutionLifecycleV1,
) -> Result<(), ExecutionError> {
    execution_records::append_event(
        &record.task_id,
        &ExecutionEventV1 {
            schema_version: 1,
            execution_id: record.execution_id.clone(),
            sequence,
            timestamp_unix_ms,
            event: ExecutionEventKindV1::Lifecycle { state },
        },
    )
    .map_err(|_| interrupted_error())
}

fn interrupted_error() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        "Interrupted execution state could not be recorded safely",
        true,
    )
}
