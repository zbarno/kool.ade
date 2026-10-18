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
    cleanup_ms: Option<u64>,
    message: &'static str,
) -> Result<(), ExecutionError> {
    let now = crate::execution::docker::backend::now_unix_ms().max(record.started_at_unix_ms);
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
    if let Some(event) = last_event.as_ref()
        && let ExecutionEventKindV1::Lifecycle { state } = &event.event
        && let Some(record_state) = terminal_record_state(*state)
    {
        reconcile_terminal_record(
            record,
            record_state,
            event,
            cleanup_complete,
            cleanup_ms,
            now,
        )?;
        return Ok(());
    }
    if !matches!(
        record.state,
        ExecutionRecordStateV1::Succeeded
            | ExecutionRecordStateV1::Failed
            | ExecutionRecordStateV1::Cancelled
            | ExecutionRecordStateV1::Interrupted
    ) {
        let failure = ExecutionError::new(ExecutionErrorKind::WorkerDisconnected, message, true);
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
            artifacts: record
                .artifacts
                .iter()
                .map(
                    |artifact| crate::execution::contract::ArtifactManifestEntryV1 {
                        relative_path: artifact.relative_path.clone(),
                        size_bytes: artifact.size_bytes,
                        sha256: artifact
                            .sha256
                            .strip_prefix("sha256:")
                            .unwrap_or(&artifact.sha256)
                            .to_owned(),
                        media_type: artifact
                            .media_type
                            .clone()
                            .unwrap_or_else(|| "application/octet-stream".into()),
                    },
                )
                .collect(),
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
        if let Some(cleanup_ms) = cleanup_ms {
            record.phase_timings.cleanup_ms = Some(cleanup_ms);
        }
        ExecutionCleanupStateV1::Complete
    } else {
        ExecutionCleanupStateV1::Pending
    };
    execution_records::save(record).map_err(|_| interrupted_error())
}

fn terminal_record_state(state: ExecutionLifecycleV1) -> Option<ExecutionRecordStateV1> {
    match state {
        ExecutionLifecycleV1::Succeeded => Some(ExecutionRecordStateV1::Succeeded),
        ExecutionLifecycleV1::Failed => Some(ExecutionRecordStateV1::Failed),
        ExecutionLifecycleV1::Cancelled => Some(ExecutionRecordStateV1::Cancelled),
        ExecutionLifecycleV1::Interrupted => Some(ExecutionRecordStateV1::Interrupted),
        ExecutionLifecycleV1::Queued
        | ExecutionLifecycleV1::Preparing
        | ExecutionLifecycleV1::WaitingForDependencyApproval
        | ExecutionLifecycleV1::WaitingForRuntime
        | ExecutionLifecycleV1::Running
        | ExecutionLifecycleV1::Collecting => None,
    }
}

fn reconcile_terminal_record(
    record: &mut ExecutionRecordV1,
    state: ExecutionRecordStateV1,
    event: &ExecutionEventV1,
    cleanup_complete: bool,
    cleanup_ms: Option<u64>,
    now: u64,
) -> Result<(), ExecutionError> {
    use crate::execution::errors::ExecutionErrorKind;

    let terminal_failure = match state {
        ExecutionRecordStateV1::Succeeded => None,
        ExecutionRecordStateV1::Failed => Some(ExecutionError::new(
            ExecutionErrorKind::ExecutionFailed,
            "Execution failed before its complete terminal record was persisted",
            false,
        )),
        ExecutionRecordStateV1::Cancelled => Some(ExecutionError::new(
            ExecutionErrorKind::Cancelled,
            "Execution was cancelled",
            false,
        )),
        ExecutionRecordStateV1::Interrupted => Some(ExecutionError::new(
            ExecutionErrorKind::WorkerDisconnected,
            "Execution was interrupted before its complete terminal record was persisted",
            true,
        )),
        _ => return Err(interrupted_error()),
    };
    let preserve_result = record.state == state
        && record
            .result
            .as_ref()
            .is_some_and(|result| result.execution_id == record.execution_id);
    let cancelled = state == ExecutionRecordStateV1::Cancelled;
    let exit_code = match state {
        ExecutionRecordStateV1::Succeeded => Some(record.exit_code.unwrap_or(0)),
        _ => record.exit_code,
    };
    let artifacts = record
        .artifacts
        .iter()
        .map(
            |artifact| crate::execution::contract::ArtifactManifestEntryV1 {
                relative_path: artifact.relative_path.clone(),
                size_bytes: artifact.size_bytes,
                sha256: artifact
                    .sha256
                    .strip_prefix("sha256:")
                    .unwrap_or(&artifact.sha256)
                    .to_owned(),
                media_type: artifact
                    .media_type
                    .clone()
                    .unwrap_or_else(|| "application/octet-stream".into()),
            },
        )
        .collect();
    record.state = state;
    record.event_cursor = event.sequence;
    record.exit_code = exit_code;
    record.ended_at_unix_ms = Some(event.timestamp_unix_ms.max(record.started_at_unix_ms));
    record.updated_at_unix_ms = now.max(record.updated_at_unix_ms);
    record.ended_at_unix_ms = Some(
        record
            .ended_at_unix_ms
            .unwrap()
            .max(record.updated_at_unix_ms),
    );
    if !preserve_result {
        record.result = Some(ExecutionResultV1 {
            schema_version: 1,
            execution_id: record.execution_id.clone(),
            exit_code,
            signal: None,
            timed_out: false,
            cancelled,
            metrics: ResourceMetricsV1 {
                cpu_millis: 0,
                peak_memory_bytes: 0,
                process_count: 0,
                disk_bytes: 0,
            },
            artifacts,
            failure: terminal_failure,
        });
    }
    record.cleanup = if cleanup_complete {
        if let Some(cleanup_ms) = cleanup_ms {
            record.phase_timings.cleanup_ms = Some(cleanup_ms);
        }
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
