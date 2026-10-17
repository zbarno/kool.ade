use std::time::{Duration, Instant};

use bollard::query_parameters::InspectContainerOptionsBuilder;

use crate::execution::{
    backend::ExecutionHandleV1,
    contract::{EventPageRequestV1, ExecutionEventKindV1, ExecutionEventV1, ExecutionLifecycleV1},
    docker::backend::DockerExecutionBackend,
    errors::{ExecutionError, ExecutionErrorKind},
};
use crate::persistence::execution_records::{self, ExecutionCleanupStateV1, ExecutionRecordV1};

pub(in crate::execution::docker::backend::lifecycle) async fn cancel_persisted(
    backend: &DockerExecutionBackend,
    handle: &ExecutionHandleV1,
    generation: u64,
) -> Result<(), ExecutionError> {
    let mut record = super::load_record(backend, handle)?;
    if is_terminal(record.state) {
        return Ok(());
    }
    if record.lease_generation != generation {
        return Err(super::invalid_request());
    }
    let _task_lock = execution_records::try_task_lock(&record.task_id)
        .map_err(|_| {
            super::internal_error_with_context("acquiring the task lock for persisted cancellation")
        })?
        .ok_or_else(|| {
            ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "Another execution already owns this task clone",
                false,
            )
        })?;
    let Some(container_id) = record
        .container_id
        .as_deref()
        .or(record.container_name.as_deref())
    else {
        super::super::recovery::mark_interrupted(
            &mut record,
            false,
            None,
            "Execution container identity was lost before cancellation",
        )?;
        return Err(super::disconnected());
    };
    let info = tokio::time::timeout(
        Duration::from_secs(5),
        backend.docker.inspect_container(
            container_id,
            Some(
                InspectContainerOptionsBuilder::default()
                    .size(false)
                    .build(),
            ),
        ),
    )
    .await
    .map_err(|_| super::operation_timeout())?
    .map_err(|_| super::disconnected())?;
    super::super::recovery::verify_record_labels(
        &record,
        &info
            .config
            .and_then(|config| config.labels)
            .unwrap_or_default(),
    )?;
    let request = record.request.as_ref().ok_or_else(super::disconnected)?;
    let stage_out_expected = super::super::execute::workspace_stage_out_enabled(request);
    let workspace_path = backend.resolver.resolve_owned_clone(&request.workspace)?;
    let workspace =
        crate::execution::docker::sandbox::ValidatedWorkspace::inspect(&workspace_path)?;
    let snapshot = record
        .workspace_manifest
        .as_deref()
        .map(|entries| {
            crate::execution::docker::sandbox::staging::WorkspaceSnapshot::from_record_entries(
                entries,
                request
                    .policy
                    .workspace_write_scope()
                    .map(|scope| std::path::PathBuf::from(&scope.root_relative)),
            )
        })
        .transpose()?
        .ok_or_else(|| {
            ExecutionError::new(
                ExecutionErrorKind::WorkerDisconnected,
                "Workspace baseline is unavailable after recovery; task clone was preserved",
                false,
            )
        })?;
    let stage_out_started = Instant::now();
    let synchronized = super::super::execute::terminate_and_sync(
        backend,
        container_id,
        &workspace,
        Some(&snapshot),
        request,
    )
    .await;
    let stage_out_ms = super::elapsed_millis(stage_out_started);
    if stage_out_expected {
        record.phase_timings.workspace_stage_out_ms = Some(stage_out_ms);
    }
    let artifacts = match synchronized {
        Ok(artifacts) => artifacts,
        Err(error) => {
            super::super::recovery::mark_interrupted(
                &mut record,
                false,
                None,
                "Cancellation could not preserve the workspace; task clone was retained",
            )?;
            return Err(error);
        }
    };
    mark_cancelled(&mut record, artifacts)?;
    execution_records::save(&record)
        .map_err(|_| super::internal_error_with_context("persisting cancelled execution state"))
}

fn mark_cancelled(
    record: &mut ExecutionRecordV1,
    artifacts: Vec<crate::execution::contract::ArtifactManifestEntryV1>,
) -> Result<(), ExecutionError> {
    let now = crate::execution::docker::backend::now_unix_ms().max(record.started_at_unix_ms);
    let cursor = execution_records::journal_cursor(&record.task_id, &record.execution_id)
        .map_err(|_| super::internal_error_with_context("reading the cancellation event cursor"))?;
    let last = if cursor > 0 {
        execution_records::load_events(
            &record.task_id,
            &record.execution_id,
            EventPageRequestV1 {
                after_sequence: cursor.saturating_sub(1),
                max_events: 1,
            },
        )
        .map_err(|_| super::internal_error_with_context("reading the latest cancellation event"))?
        .into_iter()
        .last()
    } else {
        None
    };
    let mut sequence = cursor;
    if sequence == 0 {
        sequence = 1;
        append_lifecycle(record, sequence, now, ExecutionLifecycleV1::Queued)?;
    }
    let sequence = sequence.saturating_add(1);
    append_lifecycle(
        record,
        sequence,
        last.map(|event| event.timestamp_unix_ms)
            .unwrap_or(now)
            .max(now),
        ExecutionLifecycleV1::Cancelled,
    )?;
    let failure = ExecutionError::new(
        ExecutionErrorKind::Cancelled,
        "Execution was cancelled",
        false,
    );
    record.state = execution_records::ExecutionRecordStateV1::Cancelled;
    record.updated_at_unix_ms = now;
    record.ended_at_unix_ms = Some(now);
    record.exit_code = None;
    record.artifacts = artifacts
        .iter()
        .map(
            |artifact| crate::persistence::execution_records::ExecutionArtifactV1 {
                relative_path: artifact.relative_path.clone(),
                sha256: format!("sha256:{}", artifact.sha256),
                size_bytes: artifact.size_bytes,
                media_type: Some(artifact.media_type.clone()),
            },
        )
        .collect();
    let mut result = super::result(&record.execution_id, Some(failure), None, false, true);
    result.artifacts = artifacts;
    record.result = Some(result);
    record.event_cursor = sequence;
    record.cleanup = ExecutionCleanupStateV1::Pending;
    Ok(())
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
    .map_err(|_| super::internal_error_with_context("appending a cancellation lifecycle event"))
}

fn is_terminal(state: execution_records::ExecutionRecordStateV1) -> bool {
    matches!(
        state,
        execution_records::ExecutionRecordStateV1::Succeeded
            | execution_records::ExecutionRecordStateV1::Failed
            | execution_records::ExecutionRecordStateV1::Cancelled
            | execution_records::ExecutionRecordStateV1::Interrupted
    )
}
