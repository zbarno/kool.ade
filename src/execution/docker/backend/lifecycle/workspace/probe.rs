use crate::execution::{
    backend::{BackendCapabilitiesV1, BackendProbeV1},
    contract::ExecutionTarget,
    docker::{
        backend::{DockerExecutionBackend, prerequisite_error},
        images::ImageLock,
    },
    errors::{ExecutionError, ExecutionErrorKind},
};

pub(in crate::execution::docker::backend::lifecycle) async fn probe(
    backend: &DockerExecutionBackend,
    target: &ExecutionTarget,
) -> Result<BackendProbeV1, ExecutionError> {
    if target != &ExecutionTarget::local() {
        return Ok(BackendProbeV1 {
            wire_version: 1,
            available: false,
            compatible: false,
            architecture: None,
            failure: Some(ExecutionErrorKind::WorkerIncompatible),
            capabilities: None,
        });
    }
    super::super::recovery::reconcile(backend).await?;
    let readiness = backend
        .engine
        .probe(&backend.resolver.staging_root())
        .await
        .map_err(prerequisite_error)?;
    let (current_execution_count, active_cpu_millis, active_memory_bytes) = {
        let executions = backend.executions.lock().map_err(|_| {
            crate::execution::docker::backend::internal_error_with_context(
                "counting active executions during a probe",
            )
        })?;
        let mut reserved = Some(0_u32);
        let mut reserved_memory = Some(0_u64);
        for execution in executions.values() {
            let execution = execution.lock().ok();
            match execution {
                Some(execution) if !execution.terminal => {
                    reserved = reserved.and_then(|total| {
                        total.checked_add(execution.request.resource_budget.cpu_millis)
                    });
                    reserved_memory = reserved_memory.and_then(|total| {
                        total.checked_add(execution.request.resource_budget.memory_bytes)
                    });
                }
                Some(_) => {}
                None => reserved = None,
            }
        }
        (
            u32::try_from(executions.len()).unwrap_or(u32::MAX),
            reserved,
            reserved_memory,
        )
    };
    let (pending_cpu_millis, pending_memory_bytes) = backend
        .starting
        .lock()
        .ok()
        .and_then(|starting| {
            starting
                .values()
                .try_fold((0_u32, 0_u64), |(cpu, memory), item| {
                    Some((cpu.checked_add(item.0)?, memory.checked_add(item.1)?))
                })
        })
        .map_or((None, None), |(cpu, memory)| (Some(cpu), Some(memory)));
    let reserved_cpu_millis = active_cpu_millis
        .zip(pending_cpu_millis)
        .and_then(|(active, pending)| active.checked_add(pending));
    let reserved_memory_bytes = active_memory_bytes
        .zip(pending_memory_bytes)
        .and_then(|(active, pending)| active.checked_add(pending));
    let host_resources =
        crate::execution::resources::observe_host(reserved_cpu_millis, reserved_memory_bytes);
    let capabilities = BackendCapabilitiesV1 {
        app_version: env!("CARGO_PKG_VERSION").into(),
        docker_version: readiness.daemon_version.clone(),
        docker_api_version: readiness.api_version.clone(),
        docker_rootless: readiness.rootless,
        docker_desktop: Some(readiness.docker_desktop),
        linux_containers: readiness.linux_containers,
        seccomp_enabled: readiness.seccomp_enabled,
        memory_limit: readiness.memory_limit,
        cpu_quota: readiness.cpu_quota,
        pids_limit: readiness.pids_limit,
        disk_available_bytes: readiness.disk_available_bytes,
        available_cpu_millis: host_resources.available_cpu_millis,
        available_memory_bytes: host_resources.available_memory_bytes,
        cpu_pressure_some_avg10_basis_points: host_resources.cpu_pressure_some_avg10_basis_points,
        load_millis: None,
        gpu_available: false,
        display_available: false,
        current_execution_count,
        maximum_execution_count: None,
        resource_ceiling: crate::execution::contract::ResourceBudgetV1 {
            cpu_millis: 16_000,
            memory_bytes: 64 * 1024 * 1024 * 1024,
            process_count: 4_096,
            temporary_disk_bytes: 100 * 1024 * 1024 * 1024,
            output_bytes: 1024 * 1024 * 1024,
        },
        trusted_image_digests: ImageLock::bundled()
            .map(|lock| {
                lock.profiles
                    .values()
                    .flatten()
                    .map(|image| image.digest.clone())
                    .collect()
            })
            .unwrap_or_default(),
        policy_hash: crate::execution::policy::policy_hash(),
    };
    Ok(BackendProbeV1 {
        wire_version: 1,
        available: readiness.ready,
        compatible: readiness.ready,
        architecture: readiness.architecture,
        failure: readiness
            .failure
            .map(|_| ExecutionErrorKind::EnvironmentUnavailable),
        capabilities: Some(capabilities),
    })
}
