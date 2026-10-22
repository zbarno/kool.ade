use super::*;
use crate::execution::docker::backend::TaskWorkspaceResolver;
use std::time::Duration;

pub(super) fn dispatch(
    backend: Arc<dyn ExecutionBackend>,
    stager: Option<Arc<crate::execution::remote::staging::WorkerStager>>,
    capacity: Option<&super::super::super::WorkerCapacityPool>,
    capacity_leases: Arc<Mutex<HashMap<String, super::super::super::WorkerCapacityLease>>>,
    runtime: &tokio::runtime::Runtime,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), ExecutionError> {
    let request: StartRequest = decode(&envelope.payload)?;
    require_execution_id(envelope, &request.request.execution_id)?;
    if request.lease.workspace != request.request.workspace {
        return Err(super::super::incompatible(
            "execution start does not match its staged workspace lease",
        ));
    }
    if capacity_leases
        .lock()
        .map_err(|_| super::super::incompatible("worker capacity registry is unavailable"))?
        .contains_key(&request.request.execution_id)
    {
        return Err(super::super::incompatible(
            "worker execution capacity is already reserved",
        ));
    }
    let private_files_attached = if let Some(stager) = &stager {
        stager
            .validate_private_file_workspace(
                &request.request.execution_id,
                &request.request.workspace,
            )
            .map_err(|_| {
                super::super::incompatible("private-file grant belongs to another workspace")
            })?;
        let private_mount = match stager.private_files_mount_root(&request.request.execution_id) {
            Ok(mount) => mount,
            Err(_) => {
                let _ = stager.cleanup_private_file_execution(&request.request.execution_id);
                return Err(super::super::incompatible(
                    "worker private-file grant is incomplete or expired",
                ));
            }
        };
        private_mount.is_some()
    } else {
        false
    };
    let reservation = match capacity {
        Some(pool) => {
            let capacity_cpu_millis = crate::execution::resources::effective_cpu_capacity_millis();
            let capacity_memory_bytes =
                crate::execution::resources::effective_memory_capacity_bytes();
            let reservation = capacity_cpu_millis
                .zip(capacity_memory_bytes)
                .ok_or_else(|| anyhow::anyhow!("worker CPU or memory capacity is unavailable"))
                .and_then(|(cpu, memory)| {
                    pool.acquire_resources(
                        &request.request.execution_id,
                        request.request.resource_budget.cpu_millis,
                        request.request.resource_budget.memory_bytes,
                        cpu,
                        memory,
                    )
                });
            match reservation {
                Ok(reservation) => Some(reservation),
                Err(_) => {
                    // Capacity rejection happens before the backend can start a
                    // container, so the private stage is known to be unmounted.
                    if let Some(stager) = &stager {
                        stager
                    .cleanup_private_file_execution(&request.request.execution_id)
                    .map_err(|_| {
                        super::super::incompatible(
                            "private-file stage could not be cleaned after capacity rejection",
                        )
                    })?;
                    }
                    return Err(super::super::incompatible(
                        crate::execution::errors::REMOTE_START_CAPACITY_REJECTION,
                    ));
                }
            }
        }
        None => None,
    };
    if let Some(stager) = &stager {
        stager
            .mark_private_file_execution_started(&request.request.execution_id)
            .map_err(|_| {
                super::super::incompatible("private-file stage is incomplete and cannot start")
            })?;
    }
    let mut worker_request = request.request.clone();
    worker_request.private_files_attached = private_files_attached;
    let handle = match runtime.block_on(backend.start(&request.lease, &worker_request)) {
        Ok(handle) => handle,
        Err(error) => {
            if let Some(stager) = &stager {
                stager
                    .cleanup_stopped_private_file_execution(&request.request.execution_id)
                    .map_err(|_| {
                        super::super::incompatible(
                            "private-file stage could not be cleaned after start failure",
                        )
                    })?;
            }
            return Err(error);
        }
    };
    if let Some(reservation) = reservation {
        capacity_leases
            .lock()
            .map_err(|_| super::super::incompatible("worker capacity registry is unavailable"))?
            .insert(request.request.execution_id.clone(), reservation);
    }
    if let Some(stager) = stager.clone()
        && let Ok(Some(expiry)) = stager.private_file_expiry(&handle.execution_id)
    {
        spawn_expiry_cleanup(
            backend.clone(),
            stager.clone(),
            capacity_leases.clone(),
            handle.clone(),
            request.request.cancellation_generation,
            expiry,
            runtime,
        );
    }
    if let Some(stager) = &stager
        && stager
            .bind_execution(&request.lease.lease_id, &handle.execution_id)
            .is_err()
    {
        if runtime
            .block_on(backend.cancel(&handle, request.request.cancellation_generation))
            .is_ok()
            && runtime.block_on(backend.release(&handle)).is_ok()
        {
            if let Ok(mut leases) = capacity_leases.lock() {
                leases.remove(&handle.execution_id);
            }
            let _ = stager.release_execution(&handle.execution_id);
        }
        return Err(super::super::incompatible(
            "execution workspace generation could not be reserved",
        ));
    }
    json_value(handle).map(|value| (Kind::Acknowledge, value))
}

fn spawn_expiry_cleanup(
    backend: Arc<dyn ExecutionBackend>,
    stager: Arc<crate::execution::remote::staging::WorkerStager>,
    leases: Arc<Mutex<HashMap<String, super::super::super::WorkerCapacityLease>>>,
    handle: crate::execution::backend::ExecutionHandleV1,
    cancellation_generation: u64,
    expiry: u64,
    runtime: &tokio::runtime::Runtime,
) {
    let delay = expiry.saturating_sub(crate::persistence::harness_settings::current_unix_ms());
    runtime.spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay)).await;
        loop {
            let expiry = match stager.private_file_expiry(&handle.execution_id) {
                Ok(Some(expiry)) => expiry,
                Ok(None) => break,
                Err(_) => {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            let remaining =
                expiry.saturating_sub(crate::persistence::harness_settings::current_unix_ms());
            if remaining > 0 {
                tokio::time::sleep(Duration::from_millis(remaining)).await;
                continue;
            }
            if backend
                .cancel(&handle, cancellation_generation)
                .await
                .is_ok()
                && backend.release(&handle).await.is_ok()
            {
                if let Ok(mut leases) = leases.lock() {
                    leases.remove(&handle.execution_id);
                }
                if stager.release_execution(&handle.execution_id).is_ok() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}
