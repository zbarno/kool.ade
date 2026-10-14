use std::sync::{Arc, atomic::AtomicUsize};

use crate::execution::{
    contract::{ExecutionEventV1, ExecutionLifecycleV1, ExecutionRequestV1, ExecutionResultV1},
    docker::sandbox::{ValidatedWorkspace, WorkspaceSnapshot},
};

pub(in crate::execution::docker::backend) struct RuntimeExecution {
    pub request: ExecutionRequestV1,
    pub container_id: String,
    pub command_id: Option<String>,
    pub command_started: bool,
    pub(super) container_name: String,
    pub lease_id: String,
    pub _workspace: ValidatedWorkspace,
    pub workspace_snapshot: Option<WorkspaceSnapshot>,
    pub events: Vec<ExecutionEventV1>,
    pub result: Option<ExecutionResultV1>,
    pub executed: bool,
    pub terminal: bool,
    pub cancellation_requests: Arc<AtomicUsize>,
    lifecycle_state: ExecutionLifecycleV1,
    pub output_bytes: u64,
    next_event_sequence: u64,
    owner_instance_id: String,
    policy_version: String,
    lease_expires_at_unix_ms: u64,
    started_at_unix_ms: u64,
    pub(super) cleanup_complete: bool,
    _task_lock: std::fs::File,
}

pub(super) struct RuntimeExecutionContext {
    pub container_id: String,
    pub container_name: String,
    pub workspace: ValidatedWorkspace,
    pub lease_id: String,
    pub owner_instance_id: String,
    pub policy_version: String,
    pub lease_expires_at_unix_ms: u64,
    pub task_lock: std::fs::File,
    pub workspace_snapshot: Option<WorkspaceSnapshot>,
}

mod events;

impl RuntimeExecution {
    pub(super) fn new(request: ExecutionRequestV1, context: RuntimeExecutionContext) -> Self {
        Self {
            request,
            container_id: context.container_id,
            command_id: None,
            command_started: false,
            container_name: context.container_name,
            lease_id: context.lease_id,
            _workspace: context.workspace,
            workspace_snapshot: context.workspace_snapshot,
            events: Vec::new(),
            result: None,
            executed: false,
            terminal: false,
            cancellation_requests: Arc::new(AtomicUsize::new(0)),
            lifecycle_state: ExecutionLifecycleV1::Queued,
            output_bytes: 0,
            next_event_sequence: 1,
            owner_instance_id: context.owner_instance_id,
            policy_version: context.policy_version,
            lease_expires_at_unix_ms: context.lease_expires_at_unix_ms,
            started_at_unix_ms: crate::execution::docker::backend::now_unix_ms().max(1),
            cleanup_complete: false,
            _task_lock: context.task_lock,
        }
    }

    pub(super) fn restore(
        record: &crate::persistence::execution_records::ExecutionRecordV1,
        workspace: ValidatedWorkspace,
        task_lock: std::fs::File,
    ) -> Result<Self, crate::execution::errors::ExecutionError> {
        use crate::persistence::execution_records::ExecutionRecordStateV1;

        let request = record.request.clone().ok_or_else(|| {
            crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
                "Execution record does not contain a resumable request",
                false,
            )
        })?;
        let container_name = record.container_name.clone().ok_or_else(|| {
            crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
                "Execution record does not contain a container identity",
                false,
            )
        })?;
        let lease_id = record.workspace_lease_id.clone().ok_or_else(|| {
            crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
                "Execution record does not contain a workspace lease",
                false,
            )
        })?;
        let lease_expires_at_unix_ms = record.lease_expires_at_unix_ms.ok_or_else(|| {
            crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
                "Execution record does not contain a workspace lease expiry",
                false,
            )
        })?;
        let cursor = crate::persistence::execution_records::journal_cursor(
            &record.task_id,
            &record.execution_id,
        )
        .map_err(|_| {
            crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
                "Execution event journal could not be resumed",
                false,
            )
        })?;
        let events = crate::persistence::execution_records::load_events(
            &record.task_id,
            &record.execution_id,
            crate::execution::contract::EventPageRequestV1 {
                after_sequence: cursor.saturating_sub(256),
                max_events: 256,
            },
        )
        .map_err(|_| {
            crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
                "Execution event journal could not be resumed",
                false,
            )
        })?;
        let workspace_snapshot = record
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
            .transpose()?;
        Ok(Self {
            request,
            container_id: record.container_id.clone().unwrap_or_default(),
            command_id: record.command_id.clone(),
            command_started: record.command_started,
            container_name,
            lease_id,
            _workspace: workspace,
            workspace_snapshot,
            events,
            result: record.result.clone(),
            executed: record.command_started,
            terminal: matches!(
                record.state,
                ExecutionRecordStateV1::Succeeded
                    | ExecutionRecordStateV1::Failed
                    | ExecutionRecordStateV1::Cancelled
                    | ExecutionRecordStateV1::Interrupted
            ),
            cancellation_requests: Arc::new(AtomicUsize::new(0)),
            lifecycle_state: match record.state {
                ExecutionRecordStateV1::Preparing => ExecutionLifecycleV1::Preparing,
                ExecutionRecordStateV1::Running => ExecutionLifecycleV1::Running,
                ExecutionRecordStateV1::WaitingForDependencyApproval
                | ExecutionRecordStateV1::WaitingForRuntime => ExecutionLifecycleV1::Preparing,
                ExecutionRecordStateV1::CollectingResults => ExecutionLifecycleV1::Collecting,
                ExecutionRecordStateV1::Succeeded => ExecutionLifecycleV1::Succeeded,
                ExecutionRecordStateV1::Failed => ExecutionLifecycleV1::Failed,
                ExecutionRecordStateV1::Cancelled => ExecutionLifecycleV1::Cancelled,
                ExecutionRecordStateV1::Interrupted => ExecutionLifecycleV1::Interrupted,
            },
            output_bytes: record.output_bytes,
            next_event_sequence: cursor.saturating_add(1),
            owner_instance_id: record.owner_instance_id.clone(),
            policy_version: record.policy_version.clone(),
            lease_expires_at_unix_ms,
            started_at_unix_ms: record.started_at_unix_ms,
            cleanup_complete: record.cleanup
                == crate::persistence::execution_records::ExecutionCleanupStateV1::Complete,
            _task_lock: task_lock,
        })
    }

    pub(super) fn persist(&self) -> Result<(), crate::execution::errors::ExecutionError> {
        use crate::persistence::execution_records::{
            ExecutionArtifactV1, ExecutionCleanupStateV1, ExecutionRecordStateV1, ExecutionRecordV1,
        };
        let now = crate::execution::docker::backend::now_unix_ms().max(self.started_at_unix_ms);
        let state = match self.lifecycle_state {
            ExecutionLifecycleV1::Queued | ExecutionLifecycleV1::Preparing => {
                ExecutionRecordStateV1::Preparing
            }
            ExecutionLifecycleV1::Running => ExecutionRecordStateV1::Running,
            ExecutionLifecycleV1::Collecting => ExecutionRecordStateV1::CollectingResults,
            ExecutionLifecycleV1::Succeeded => ExecutionRecordStateV1::Succeeded,
            ExecutionLifecycleV1::Failed => ExecutionRecordStateV1::Failed,
            ExecutionLifecycleV1::Cancelled => ExecutionRecordStateV1::Cancelled,
            ExecutionLifecycleV1::Interrupted => ExecutionRecordStateV1::Interrupted,
        };
        let record = ExecutionRecordV1 {
            schema_version: 1,
            execution_id: self.request.execution_id.clone(),
            request: Some(self.request.clone()),
            project_id: self.request.project_id.clone(),
            task_id: self.request.task_id.clone(),
            target_id: self.request.target.target_id.clone(),
            controller_target_id: self.request.controller_target_id.clone(),
            owner_instance_id: self.owner_instance_id.clone(),
            container_name: Some(self.container_name.clone()),
            workspace_lease_id: Some(self.lease_id.clone()),
            workspace_source_sha: self.request.workspace.source_sha.clone(),
            destination_sha: None,
            workspace_manifest: self
                .workspace_snapshot
                .as_ref()
                .map(|snapshot| snapshot.record_entries()),
            container_id: (!self.container_id.is_empty()).then(|| self.container_id.clone()),
            command_id: self.command_id.clone(),
            command_started: self.command_started,
            result: self.result.clone(),
            image_digest: self.request.image_digest.clone(),
            policy_version: self.policy_version.clone(),
            lease_generation: self.request.cancellation_generation,
            lease_expires_at_unix_ms: Some(self.lease_expires_at_unix_ms),
            attempt: u32::try_from(self.request.request_sequence)
                .unwrap_or(u32::MAX)
                .max(1),
            state,
            event_cursor: self.next_event_sequence.saturating_sub(1),
            output_bytes: self.output_bytes,
            started_at_unix_ms: self.started_at_unix_ms,
            updated_at_unix_ms: now,
            ended_at_unix_ms: self.terminal.then_some(now),
            exit_code: self.result.as_ref().and_then(|result| result.exit_code),
            artifacts: self.result.as_ref().map_or_else(Vec::new, |result| {
                result
                    .artifacts
                    .iter()
                    .map(|artifact| ExecutionArtifactV1 {
                        relative_path: artifact.relative_path.clone(),
                        sha256: artifact.sha256.clone(),
                        size_bytes: artifact.size_bytes,
                    })
                    .collect()
            }),
            cleanup: if self.cleanup_complete {
                ExecutionCleanupStateV1::Complete
            } else {
                ExecutionCleanupStateV1::Pending
            },
        };
        crate::persistence::execution_records::save(&record).map_err(|error| {
            let detail = if error.starts_with("cannot atomically write ") {
                error
                    .rsplit_once(": ")
                    .map(|(_, kind)| format!("atomic write failed ({kind})"))
                    .unwrap_or_else(|| "atomic write failed".into())
            } else {
                error.clone()
            };
            crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::ExecutionFailed,
                format!(
                    "Execution state could not be persisted ({detail}); task execution cannot continue"
                ),
                false,
            )
        })
    }
}

#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;
