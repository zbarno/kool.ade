use std::time::Duration;

use bollard::query_parameters::InspectContainerOptionsBuilder;

use crate::execution::{
    docker::{
        backend::{DockerExecutionBackend, now_unix_ms},
        sandbox::{
            EXECUTION_LABEL, LEASE_LABEL, OWNER_LABEL, POLICY_LABEL, TASK_LABEL, WORKSPACE_LABEL,
        },
    },
    errors::{ExecutionError, ExecutionErrorKind},
};
use crate::persistence::execution_records::{self, ExecutionRecordV1};

const RPC_TIMEOUT: Duration = Duration::from_secs(5);

mod interrupt;
mod restore;
pub(super) use interrupt::mark_interrupted;

pub(super) async fn reconcile(backend: &DockerExecutionBackend) -> Result<(), ExecutionError> {
    let records = execution_records::unfinished_for_owner(&backend.identity.owner_instance_id)
        .map_err(|_| recovery_error("Durable execution state could not be read"))?;
    for mut record in records {
        let task_lock = execution_records::try_task_lock(&record.task_id)
            .map_err(|_| recovery_error("Task execution ownership could not be checked"))?;
        let Some(task_lock) = task_lock else {
            continue;
        };
        if record.target_id != "local" {
            return Err(recovery_error(
                "A pending execution targets an unsupported recovery backend",
            ));
        }
        if lease_expired(&record, now_unix_ms())? {
            reconcile_record(backend, &mut record).await?;
        } else {
            restore::restore_record(backend, &mut record, task_lock).await?;
        }
    }
    Ok(())
}

async fn reconcile_record(
    backend: &DockerExecutionBackend,
    record: &mut ExecutionRecordV1,
) -> Result<(), ExecutionError> {
    let name = record
        .container_id
        .as_deref()
        .or(record.container_name.as_deref());
    let mut removed = true;
    if let Some(name) = name {
        let inspect_options = InspectContainerOptionsBuilder::default()
            .size(false)
            .build();
        let inspected = tokio::time::timeout(
            RPC_TIMEOUT,
            backend
                .docker
                .inspect_container(name, Some(inspect_options)),
        )
        .await
        .map_err(|_| recovery_error("Docker recovery inspection timed out"))?;
        match inspected {
            Ok(info) => {
                verify_record_labels(
                    record,
                    &info
                        .config
                        .and_then(|config| config.labels)
                        .unwrap_or_default(),
                )?;
                // Recovery runs only when the task lock proves no live app instance
                // owns this execution. After verifying every ownership label, a
                // forced removal is idempotent for both running and exited containers.
                removed = tokio::time::timeout(
                    RPC_TIMEOUT,
                    backend.docker.remove_container(
                        name,
                        Some(bollard::query_parameters::RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    ),
                )
                .await
                .is_ok_and(|result| result.is_ok());
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(_) => return Err(recovery_error("Docker recovery inspection failed")),
        }
    }

    mark_interrupted(
        record,
        removed,
        "Worker restarted while execution was in flight",
    )
}

pub(super) fn verify_record_labels(
    record: &ExecutionRecordV1,
    labels: &std::collections::HashMap<String, String>,
) -> Result<(), ExecutionError> {
    let workspace = format!("{}:{}", record.project_id, record.task_id);
    if labels.get(OWNER_LABEL).map(String::as_str) != Some(&record.owner_instance_id)
        || labels.get(EXECUTION_LABEL).map(String::as_str) != Some(&record.execution_id)
        || labels.get(TASK_LABEL).map(String::as_str) != Some(&record.task_id)
        || labels.get(WORKSPACE_LABEL).map(String::as_str) != Some(&workspace)
        || labels.get(POLICY_LABEL).map(String::as_str) != Some(&record.policy_version)
        || labels
            .get(LEASE_LABEL)
            .and_then(|value| value.parse::<u64>().ok())
            != record.lease_expires_at_unix_ms
    {
        return Err(recovery_error(
            "A pending container does not match its durable Kool.ad/e ownership record",
        ));
    }
    Ok(())
}

fn lease_expired(record: &ExecutionRecordV1, now: u64) -> Result<bool, ExecutionError> {
    let Some(expires_at) = record.lease_expires_at_unix_ms else {
        return Err(recovery_error(
            "A pending execution has no durable lease expiry; recovery will not remove its container",
        ));
    };
    Ok(expires_at <= now)
}

fn recovery_error(message: &str) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::WorkerDisconnected, message, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::execution_records::{
        ExecutionCleanupStateV1, ExecutionRecordStateV1, ExecutionRecordV1,
    };
    use std::collections::HashMap;

    fn record() -> ExecutionRecordV1 {
        ExecutionRecordV1 {
            schema_version: 1,
            execution_id: "00000000-0000-4000-8000-000000000001".into(),
            request: None,
            project_id: "00000000-0000-4000-8000-000000000002".into(),
            task_id: "00000000-0000-4000-8000-000000000003".into(),
            target_id: "local".into(),
            controller_target_id: None,
            owner_instance_id: "00000000-0000-4000-8000-000000000004".into(),
            container_name: Some("koolade-exec-00000000-0000-4000-8000-000000000001".into()),
            workspace_lease_id: None,
            workspace_source_sha: "a".repeat(40),
            destination_sha: None,
            workspace_manifest: None,
            container_id: None,
            command_id: None,
            command_started: false,
            result: None,
            image_digest: format!("sha256:{}", "b".repeat(64)),
            policy_version: "docker-sandbox-v1".into(),
            lease_generation: 1,
            lease_expires_at_unix_ms: Some(2_000),
            attempt: 1,
            state: ExecutionRecordStateV1::Running,
            event_cursor: 2,
            output_bytes: 0,
            started_at_unix_ms: 1_000,
            updated_at_unix_ms: 1_001,
            ended_at_unix_ms: None,
            exit_code: None,
            artifacts: Vec::new(),
            cleanup: ExecutionCleanupStateV1::Pending,
        }
    }

    fn matching_labels(record: &ExecutionRecordV1) -> HashMap<String, String> {
        HashMap::from([
            (OWNER_LABEL.into(), record.owner_instance_id.clone()),
            (EXECUTION_LABEL.into(), record.execution_id.clone()),
            (TASK_LABEL.into(), record.task_id.clone()),
            (
                WORKSPACE_LABEL.into(),
                format!("{}:{}", record.project_id, record.task_id),
            ),
            (POLICY_LABEL.into(), record.policy_version.clone()),
            (
                LEASE_LABEL.into(),
                record.lease_expires_at_unix_ms.unwrap().to_string(),
            ),
        ])
    }

    #[test]
    fn recovery_requires_a_complete_matching_ownership_label_set() {
        let record = record();
        let labels = matching_labels(&record);
        verify_record_labels(&record, &labels).unwrap();

        for key in [
            OWNER_LABEL,
            EXECUTION_LABEL,
            TASK_LABEL,
            WORKSPACE_LABEL,
            POLICY_LABEL,
            LEASE_LABEL,
        ] {
            let mut mismatched = labels.clone();
            mismatched.insert(key.into(), "different-identity".into());
            assert!(
                verify_record_labels(&record, &mismatched).is_err(),
                "mismatched ownership label {key} must be rejected"
            );
        }

        for key in [
            OWNER_LABEL,
            EXECUTION_LABEL,
            TASK_LABEL,
            WORKSPACE_LABEL,
            POLICY_LABEL,
            LEASE_LABEL,
        ] {
            let mut incomplete = labels.clone();
            incomplete.remove(key);
            assert!(
                verify_record_labels(&record, &incomplete).is_err(),
                "missing ownership label {key} must be rejected"
            );
        }
    }

    #[test]
    fn recovery_waits_for_the_durable_lease_and_fails_closed_without_one() {
        let record = record();
        assert!(!lease_expired(&record, 1_999).unwrap());
        assert!(lease_expired(&record, 2_000).unwrap());

        let mut legacy = record;
        legacy.lease_expires_at_unix_ms = None;
        assert!(lease_expired(&legacy, u64::MAX).is_err());
    }
}
