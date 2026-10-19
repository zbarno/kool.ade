use std::sync::{Arc, atomic::AtomicUsize};

use crate::execution::{
    contract::{
        ArtifactManifestEntryV1, ExecutionEventV1, ExecutionLifecycleV1, ExecutionRequestV1,
        ExecutionResultV1,
    },
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
    pub artifacts: Vec<ArtifactManifestEntryV1>,
    pub executed: bool,
    pub terminal: bool,
    pub cancellation_requests: Arc<AtomicUsize>,
    pub workspace_sync: Arc<tokio::sync::Mutex<()>>,
    lifecycle_state: ExecutionLifecycleV1,
    pub output_bytes: u64,
    pub(super) docker_usage: crate::persistence::execution_records::DockerResourceUsageV1,
    pub(super) phase_timings: crate::persistence::execution_records::ExecutionPhaseTimingsV1,
    #[cfg(test)]
    pub(in crate::execution::docker::backend) diagnostic_progress: std::sync::atomic::AtomicU8,
    #[cfg(test)]
    pub(in crate::execution::docker::backend) diagnostic_metrics: std::sync::atomic::AtomicU16,
    next_event_sequence: u64,
    owner_instance_id: String,
    policy_version: String,
    lease_expires_at_unix_ms: u64,
    started_at_unix_ms: u64,
    pub(super) cleanup_complete: bool,
    _task_lock: std::fs::File,
    pub(super) host_capacity_lease: Option<crate::execution::remote::WorkerCapacityLease>,
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
mod persist;

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
            artifacts: Vec::new(),
            executed: false,
            terminal: false,
            cancellation_requests: Arc::new(AtomicUsize::new(0)),
            workspace_sync: Arc::new(tokio::sync::Mutex::new(())),
            lifecycle_state: ExecutionLifecycleV1::Queued,
            output_bytes: 0,
            docker_usage: crate::persistence::execution_records::DockerResourceUsageV1::default(),
            phase_timings: crate::persistence::execution_records::ExecutionPhaseTimingsV1::default(
            ),
            #[cfg(test)]
            diagnostic_progress: std::sync::atomic::AtomicU8::new(0),
            #[cfg(test)]
            diagnostic_metrics: std::sync::atomic::AtomicU16::new(0),
            next_event_sequence: 1,
            owner_instance_id: context.owner_instance_id,
            policy_version: context.policy_version,
            lease_expires_at_unix_ms: context.lease_expires_at_unix_ms,
            started_at_unix_ms: crate::execution::docker::backend::now_unix_ms().max(1),
            cleanup_complete: false,
            _task_lock: context.task_lock,
            host_capacity_lease: None,
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
        let docker_usage = record.docker_usage.clone();
        let artifacts = record.result.as_ref().map_or_else(
            || {
                record
                    .artifacts
                    .iter()
                    .map(|artifact| ArtifactManifestEntryV1 {
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
                    })
                    .collect()
            },
            |result| result.artifacts.clone(),
        );
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
            artifacts,
            executed: record.command_started,
            terminal: matches!(
                record.state,
                ExecutionRecordStateV1::Succeeded
                    | ExecutionRecordStateV1::Failed
                    | ExecutionRecordStateV1::Cancelled
                    | ExecutionRecordStateV1::Interrupted
            ),
            cancellation_requests: Arc::new(AtomicUsize::new(0)),
            workspace_sync: Arc::new(tokio::sync::Mutex::new(())),
            lifecycle_state: match record.state {
                ExecutionRecordStateV1::Preparing => ExecutionLifecycleV1::Preparing,
                ExecutionRecordStateV1::WaitingForDependencyApproval => {
                    ExecutionLifecycleV1::WaitingForDependencyApproval
                }
                ExecutionRecordStateV1::WaitingForRuntime => {
                    ExecutionLifecycleV1::WaitingForRuntime
                }
                ExecutionRecordStateV1::Running => ExecutionLifecycleV1::Running,
                ExecutionRecordStateV1::CollectingResults => ExecutionLifecycleV1::Collecting,
                ExecutionRecordStateV1::Succeeded => ExecutionLifecycleV1::Succeeded,
                ExecutionRecordStateV1::Failed => ExecutionLifecycleV1::Failed,
                ExecutionRecordStateV1::Cancelled => ExecutionLifecycleV1::Cancelled,
                ExecutionRecordStateV1::Interrupted => ExecutionLifecycleV1::Interrupted,
            },
            output_bytes: record.output_bytes,
            docker_usage,
            phase_timings: record.phase_timings.clone(),
            #[cfg(test)]
            diagnostic_progress: std::sync::atomic::AtomicU8::new(0),
            #[cfg(test)]
            diagnostic_metrics: std::sync::atomic::AtomicU16::new(0),
            next_event_sequence: cursor.saturating_add(1),
            owner_instance_id: record.owner_instance_id.clone(),
            policy_version: record.policy_version.clone(),
            lease_expires_at_unix_ms,
            started_at_unix_ms: record.started_at_unix_ms,
            cleanup_complete: record.cleanup
                == crate::persistence::execution_records::ExecutionCleanupStateV1::Complete,
            _task_lock: task_lock,
            host_capacity_lease: None,
        })
    }
}

#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;
