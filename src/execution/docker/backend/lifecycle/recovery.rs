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
#[cfg(test)]
pub(super) use restore::reconcile_missing_container_404;

pub(super) async fn reconcile(backend: &DockerExecutionBackend) -> Result<(), ExecutionError> {
    let records = execution_records::unfinished_for_owner(&backend.identity.owner_instance_id)
        .map_err(|_| recovery_error("Durable execution state could not be read"))?;
    for mut record in records {
        let private_mount_recovery =
            crate::execution::remote::staging::lock_private_file_execution_for_recovery(
                &crate::persistence::state_root(),
                &record.execution_id,
            )
            .map_err(|_| recovery_error("Private-file recovery state could not be read"))?;
        if matches!(
            &private_mount_recovery,
            crate::execution::remote::staging::PrivateFileExecutionRecovery::Active
        ) {
            continue;
        }
        let private_mount_needs_recovery = matches!(
            &private_mount_recovery,
            crate::execution::remote::staging::PrivateFileExecutionRecovery::Orphaned { .. }
        );
        // Keep an orphaned mount's owner lock until the container and durable
        // execution record have both been reconciled.
        let _private_mount_recovery = private_mount_recovery;
        let task_lock = execution_records::try_task_lock(&record.task_id)
            .map_err(|_| recovery_error("Task execution ownership could not be checked"))?;
        let Some(task_lock) = task_lock else {
            if private_mount_needs_recovery {
                return Err(recovery_error(
                    "A private-file execution is still owned and cannot be safely recovered",
                ));
            }
            continue;
        };
        if record.target_id != "local" {
            return Err(recovery_error(
                "A pending execution targets an unsupported recovery backend",
            ));
        }
        if private_mount_needs_recovery || lease_expired(&record, now_unix_ms())? {
            reconcile_record(backend, &mut record, private_mount_needs_recovery).await?;
        } else {
            restore::restore_record(backend, &mut record, task_lock).await?;
        }
    }
    Ok(())
}

async fn reconcile_record(
    backend: &DockerExecutionBackend,
    record: &mut ExecutionRecordV1,
    require_container_stopped: bool,
) -> Result<(), ExecutionError> {
    let cleanup_started = std::time::Instant::now();
    let name = record
        .container_id
        .as_deref()
        .or(record.container_name.as_deref());
    let mut removed = name.is_some();
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

    if require_container_stopped && !removed {
        return Err(recovery_error(
            "A private-file container could not be confirmed stopped; its staged files were retained",
        ));
    }

    mark_interrupted(
        record,
        removed,
        removed.then(|| {
            cleanup_started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64
        }),
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
#[path = "recovery/tests.rs"]
mod tests;
