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
    let current_execution_count = u32::try_from(
        backend
            .executions
            .lock()
            .map_err(|_| {
                crate::execution::docker::backend::internal_error_with_context(
                    "counting active executions during a probe",
                )
            })?
            .len(),
    )
    .unwrap_or(u32::MAX);
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
