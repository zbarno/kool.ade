use std::time::Instant;

use bollard::query_parameters::CreateContainerOptionsBuilder;

use super::extract_archive;
use crate::execution::{
    backend::{ExecutionHandleV1, WorkspaceLeaseV1},
    contract::{ExecutionRequestV1, ExecutionTarget},
    docker::{
        backend::{
            DockerExecutionBackend, internal_error_with_context, invalid_request, now_unix_ms,
            selected_image,
        },
        sandbox::{create_body_with_private_files, staging},
    },
    errors::{ExecutionError, ExecutionErrorKind},
};

use self::start_helpers::{deadline_error, deadline_instant, engine_operation_error};
use super::cleanup::{cleanup_created_container, cleanup_named_container};
use reservation::WorkspaceStartReservation;
mod cache;
mod execution;
mod reservation;
mod start_helpers;
#[cfg(test)]
pub(in crate::execution::docker::backend::lifecycle) use execution::{
    persist_runtime_wait_error, record_start_registration_failure,
};

pub(in crate::execution::docker::backend::lifecycle) async fn start(
    backend: &DockerExecutionBackend,
    lease: &WorkspaceLeaseV1,
    request: &ExecutionRequestV1,
) -> Result<ExecutionHandleV1, ExecutionError> {
    request
        .validate_at(now_unix_ms())
        .map_err(|_| invalid_request())?;
    if request.target != ExecutionTarget::local() {
        return Err(invalid_request());
    }
    let mut admission = backend.reserve_start(
        &request.execution_id,
        request.resource_budget.cpu_millis,
        request.resource_budget.memory_bytes,
    )?;
    let task_lock = crate::persistence::execution_records::try_task_lock(&request.task_id)
        .map_err(|_| internal_error_with_context("acquiring the task execution lock"))?
        .ok_or_else(|| {
            ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "Another execution already owns this task clone",
                false,
            )
        })?;
    if lease.workspace != request.workspace
        || lease.expires_unix_ms <= now_unix_ms()
        || lease.lease_id.is_empty()
    {
        return Err(invalid_request());
    }
    let (prepared, workspace) = {
        let workspaces = backend
            .workspaces
            .lock()
            .map_err(|_| internal_error_with_context("reading the prepared workspace registry"))?;
        let prepared = workspaces
            .get(&lease.lease_id)
            .cloned()
            .ok_or_else(invalid_request)?;
        let mut state = prepared
            .lock()
            .map_err(|_| internal_error_with_context("reading prepared workspace state"))?;
        if state.workspace != request.workspace
            || state.expires_unix_ms != lease.expires_unix_ms
            || state.expires_unix_ms <= now_unix_ms()
            || state.starting
            || state.active_execution_id.is_some()
        {
            return Err(invalid_request());
        }
        state.starting = true;
        let workspace = state.verified.clone();
        drop(state);
        (prepared.clone(), workspace)
    };
    let mut reservation = WorkspaceStartReservation::new(prepared);
    workspace.revalidate()?;
    let reviewed_source = request.policy.requires_reviewed_source_checkout();
    let writable = super::super::execute::workspace_write_enabled(request);
    let snapshot = if reviewed_source {
        staging::capture_reviewed(
            &workspace,
            &request.workspace.source_sha,
            request.policy.source_input_paths(),
            request
                .policy
                .workspace_write_scope()
                .map(|scope| scope.root_relative.as_str()),
            request.resource_budget.temporary_disk_bytes,
            !writable,
        )?
    } else if let Some(scope) = request.policy.workspace_write_scope() {
        staging::capture_scoped(
            &workspace,
            &scope.root_relative,
            request.resource_budget.temporary_disk_bytes,
        )?
    } else {
        staging::capture(
            &workspace,
            request.resource_budget.temporary_disk_bytes,
            !writable,
        )?
    };
    let deadline = deadline_instant(request.deadline_unix_ms);
    let mut identity = backend.identity.clone();
    identity.lease_expires_unix_ms = request.deadline_unix_ms.saturating_add(60_000);
    let name = format!("koolade-exec-{}", request.execution_id);
    let (readiness, task_lock) = execution::ensure_runtime_ready(
        backend,
        lease,
        request,
        &workspace,
        &snapshot,
        &name,
        identity.lease_expires_unix_ms,
        task_lock,
        deadline,
    )
    .await?;
    let image_preparation_started = Instant::now();
    let image = selected_image(
        backend,
        &request.image_digest,
        readiness
            .architecture
            .as_deref()
            .ok_or_else(invalid_request)?,
    )?;
    let derived_approval =
        tokio::time::timeout_at(deadline, backend.prepare_selected_image(&image))
            .await
            .map_err(|_| deadline_error())??;
    let image_preparation_ms = start_helpers::elapsed_millis(image_preparation_started);
    let private_files_root = backend
        .resolver
        .private_files_mount_root(&request.execution_id)?;
    let body = create_body_with_private_files(
        request,
        &workspace,
        &identity,
        &image,
        derived_approval.as_ref(),
        private_files_root.as_deref(),
    )?;
    let mut execution = execution::new_execution(
        backend,
        lease,
        request,
        workspace.clone(),
        &snapshot,
        name.clone(),
        identity.lease_expires_unix_ms,
        task_lock,
    )?;
    execution.host_capacity_lease = admission.take_host_capacity_lease();
    execution.phase_timings.image_preparation_ms = Some(image_preparation_ms);
    let options = CreateContainerOptionsBuilder::default().name(&name).build();
    let created = match tokio::time::timeout_at(
        deadline,
        backend.docker.create_container(Some(options), body),
    )
    .await
    {
        Ok(Ok(created)) => created,
        Ok(Err(error)) => {
            cleanup_named_container(backend, &name, &request.execution_id).await;
            let error = engine_operation_error("create container", error);
            execution.fail_start(error.clone())?;
            return Err(error);
        }
        Err(_) => {
            cleanup_named_container(backend, &name, &request.execution_id).await;
            let error = deadline_error();
            execution.fail_start(error.clone())?;
            return Err(error);
        }
    };
    if created.id.is_empty() {
        cleanup_named_container(backend, &name, &request.execution_id).await;
        let error = super::super::disconnected();
        execution.fail_start(error.clone())?;
        return Err(error);
    }
    execution.container_id = created.id.clone();
    if let Err(error) = execution.persist() {
        cleanup_created_container(backend, &created.id, &request.execution_id).await;
        return Err(error);
    }
    if let Err(error) = workspace.revalidate() {
        cleanup_created_container(backend, &created.id, &request.execution_id).await;
        execution.fail_start(error.clone())?;
        return Err(error);
    }
    match tokio::time::timeout_at(deadline, backend.docker.start_container(&created.id, None)).await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            cleanup_created_container(backend, &created.id, &request.execution_id).await;
            let error = engine_operation_error("start container", error);
            execution.fail_start(error.clone())?;
            return Err(error);
        }
        Err(_) => {
            cleanup_created_container(backend, &created.id, &request.execution_id).await;
            let error = deadline_error();
            execution.fail_start(error.clone())?;
            return Err(error);
        }
    }
    let dependency_cache_started = Instant::now();
    let dependency_cache = tokio::time::timeout_at(
        deadline,
        cache::install_dependency_caches(backend, &created.id, request),
    )
    .await;
    execution.phase_timings.dependency_cache_stage_ms =
        Some(start_helpers::elapsed_millis(dependency_cache_started));
    if let Err(error) = execution.persist() {
        cleanup_created_container(backend, &created.id, &request.execution_id).await;
        return Err(error);
    }
    match dependency_cache {
        Ok(true) => {}
        result => {
            cleanup_created_container(backend, &created.id, &request.execution_id).await;
            let error = if result.is_err() {
                deadline_error()
            } else {
                crate::execution::docker::backend::invalid_request()
            };
            execution.fail_start(error.clone())?;
            return Err(error);
        }
    }
    let stage_in_started = Instant::now();
    let upload = tokio::time::timeout_at(
        deadline,
        extract_archive(
            backend,
            &created.id,
            "/workspace",
            &snapshot.archive,
            !writable,
        ),
    )
    .await;
    execution.phase_timings.workspace_stage_in_ms =
        Some(start_helpers::elapsed_millis(stage_in_started));
    if let Err(error) = execution.persist() {
        cleanup_created_container(backend, &created.id, &request.execution_id).await;
        return Err(error);
    }
    match upload {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            cleanup_created_container(backend, &created.id, &request.execution_id).await;
            execution.fail_start(error.clone())?;
            return Err(error);
        }
        Err(_) => {
            cleanup_created_container(backend, &created.id, &request.execution_id).await;
            let error = deadline_error();
            execution.fail_start(error.clone())?;
            return Err(error);
        }
    }
    let handle = ExecutionHandleV1 {
        execution_id: request.execution_id.clone(),
        task_id: request.task_id.clone(),
        target: request.target.clone(),
        request_sequence: request.request_sequence,
    };
    if let Err(failure) =
        execution::register_started_execution(backend, &handle, execution, &mut reservation)
    {
        let (mut execution, error) = *failure;
        cleanup_created_container(backend, &created.id, &request.execution_id).await;
        return Err(execution::record_start_registration_failure(
            &mut execution,
            error,
        ));
    }
    Ok(handle)
}
