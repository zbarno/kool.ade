use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use super::super::runtime::RuntimeExecution;
use crate::execution::{
    contract::ExecutionLifecycleV1,
    docker::backend::{DockerExecutionBackend, now_unix_ms},
    errors::{ExecutionError, ExecutionErrorKind},
};

use super::state::{disconnected, finish};

const CLEANUP_RPC_TIMEOUT: Duration = Duration::from_secs(1);
const STOP_RPC_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) async fn deadline_failure(
    backend: &DockerExecutionBackend,
    execution: &Arc<Mutex<RuntimeExecution>>,
    container_id: &str,
    execution_id: &str,
) -> Result<(), ExecutionError> {
    let cleanup = stop_owned(backend, container_id, execution_id).await;
    let failure = cleanup.as_ref().err().cloned().unwrap_or_else(|| {
        ExecutionError::new(
            ExecutionErrorKind::DeadlineExceeded,
            "Execution exceeded its deadline",
            false,
        )
    });
    finish(
        execution,
        if cleanup.is_ok() {
            ExecutionLifecycleV1::Failed
        } else {
            ExecutionLifecycleV1::Interrupted
        },
        Some(failure),
        None,
        true,
        false,
    )?;
    Ok(())
}

pub(super) fn remaining(deadline_unix_ms: u64) -> Duration {
    Duration::from_millis(deadline_unix_ms.saturating_sub(now_unix_ms()))
}

pub(super) async fn stop_owned(
    backend: &DockerExecutionBackend,
    container_id: &str,
    execution_id: &str,
) -> Result<(), ExecutionError> {
    tokio::time::timeout(
        CLEANUP_RPC_TIMEOUT,
        super::super::control::verify_owned(backend, container_id, execution_id),
    )
    .await
    .map_err(|_| disconnected())??;

    let stopped = tokio::time::timeout(
        STOP_RPC_TIMEOUT,
        backend.docker.stop_container(
            container_id,
            Some(bollard::query_parameters::StopContainerOptions {
                t: Some(1),
                ..Default::default()
            }),
        ),
    )
    .await
    .is_ok_and(|result| result.is_ok());
    if stopped {
        return Ok(());
    }

    tokio::time::timeout(
        CLEANUP_RPC_TIMEOUT,
        backend.docker.kill_container(
            container_id,
            None::<bollard::query_parameters::KillContainerOptions>,
        ),
    )
    .await
    .map_err(|_| disconnected())?
    .map_err(|_| disconnected())?;
    Ok(())
}
