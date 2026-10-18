use super::RuntimeExecution;

impl RuntimeExecution {
    pub(in crate::execution::docker::backend::lifecycle) fn persist(
        &self,
    ) -> Result<(), crate::execution::errors::ExecutionError> {
        use crate::persistence::execution_records::{
            ExecutionArtifactV1, ExecutionCleanupStateV1, ExecutionRecordStateV1, ExecutionRecordV1,
        };
        let now = crate::execution::docker::backend::now_unix_ms().max(self.started_at_unix_ms);
        let state = match self.lifecycle_state {
            crate::execution::contract::ExecutionLifecycleV1::Queued
            | crate::execution::contract::ExecutionLifecycleV1::Preparing => {
                ExecutionRecordStateV1::Preparing
            }
            crate::execution::contract::ExecutionLifecycleV1::WaitingForDependencyApproval => {
                ExecutionRecordStateV1::WaitingForDependencyApproval
            }
            crate::execution::contract::ExecutionLifecycleV1::WaitingForRuntime => {
                ExecutionRecordStateV1::WaitingForRuntime
            }
            crate::execution::contract::ExecutionLifecycleV1::Running => {
                ExecutionRecordStateV1::Running
            }
            crate::execution::contract::ExecutionLifecycleV1::Collecting => {
                ExecutionRecordStateV1::CollectingResults
            }
            crate::execution::contract::ExecutionLifecycleV1::Succeeded => {
                ExecutionRecordStateV1::Succeeded
            }
            crate::execution::contract::ExecutionLifecycleV1::Failed => {
                ExecutionRecordStateV1::Failed
            }
            crate::execution::contract::ExecutionLifecycleV1::Cancelled => {
                ExecutionRecordStateV1::Cancelled
            }
            crate::execution::contract::ExecutionLifecycleV1::Interrupted => {
                ExecutionRecordStateV1::Interrupted
            }
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
            destination_sha: self.request.destination_sha.clone(),
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
            artifacts: self
                .artifacts
                .iter()
                .map(|artifact| ExecutionArtifactV1 {
                    relative_path: artifact.relative_path.clone(),
                    sha256: format!("sha256:{}", artifact.sha256),
                    size_bytes: artifact.size_bytes,
                    media_type: Some(artifact.media_type.clone()),
                })
                .collect(),
            docker_usage: self.docker_usage.clone(),
            phase_timings: self.phase_timings.clone(),
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
