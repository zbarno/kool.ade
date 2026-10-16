use std::sync::Arc;

use bollard::query_parameters::CreateContainerOptionsBuilder;

use super::super::runtime::{RuntimeExecution, RuntimeExecutionContext};
use super::extract_archive;
use crate::execution::{
    backend::{ExecutionHandleV1, WorkspaceLeaseV1},
    contract::{ExecutionEventKindV1, ExecutionLifecycleV1, ExecutionRequestV1, ExecutionTarget},
    docker::{
        backend::{
            DockerExecutionBackend, internal_error_with_context, invalid_request, now_unix_ms,
            selected_image,
        },
        sandbox::{create_body_with_derived_approval, staging},
    },
    errors::{ExecutionError, ExecutionErrorKind},
};

use self::start_helpers::{deadline_error, deadline_instant, engine_operation_error};
use super::cleanup::{cleanup_created_container, cleanup_named_container};
mod cache;
mod start_helpers;

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
    let _reservation = backend.reserve_start(&request.execution_id)?;
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
    let writable = super::super::execute::workspace_write_enabled(request);
    let snapshot = if let Some(scope) = request.policy.workspace_write_scope() {
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
    let readiness = tokio::time::timeout_at(deadline, super::ensure_ready(backend, &workspace))
        .await
        .map_err(|_| deadline_error())??;
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
    let mut identity = backend.identity.clone();
    identity.lease_expires_unix_ms = request.deadline_unix_ms.saturating_add(60_000);
    let body = create_body_with_derived_approval(
        request,
        &workspace,
        &identity,
        &image,
        derived_approval.as_ref(),
    )?;
    let name = format!("koolade-exec-{}", request.execution_id);
    let mut execution = RuntimeExecution::new(
        request.clone(),
        RuntimeExecutionContext {
            container_id: String::new(),
            container_name: name.clone(),
            workspace: workspace.clone(),
            lease_id: lease.lease_id.clone(),
            owner_instance_id: backend.identity.owner_instance_id.clone(),
            policy_version: backend.identity.policy_version.clone(),
            lease_expires_at_unix_ms: identity.lease_expires_unix_ms,
            task_lock,
            workspace_snapshot: Some(crate::execution::docker::sandbox::WorkspaceSnapshot {
                archive: Vec::new(),
                manifest: snapshot.manifest.clone(),
                scope_root: snapshot.scope_root.clone(),
            }),
        },
    );
    execution.persist()?;
    execution.push(ExecutionEventKindV1::Lifecycle {
        state: ExecutionLifecycleV1::Queued,
    })?;
    execution.push(ExecutionEventKindV1::Lifecycle {
        state: ExecutionLifecycleV1::Preparing,
    })?;
    let options = CreateContainerOptionsBuilder::default().name(&name).build();
    let created = match tokio::time::timeout_at(
        deadline,
        backend.docker.create_container(Some(options), body),
    )
    .await
    {
        Ok(Ok(created)) => created,
        Ok(Err(error)) => {
            return Err(engine_operation_error("create container", error));
        }
        Err(_) => {
            cleanup_named_container(backend, &name, &request.execution_id).await;
            return Err(deadline_error());
        }
    };
    if created.id.is_empty() {
        cleanup_named_container(backend, &name, &request.execution_id).await;
        return Err(super::super::disconnected());
    }
    execution.container_id = created.id.clone();
    if let Err(error) = execution.persist() {
        cleanup_created_container(backend, &created.id, &request.execution_id).await;
        return Err(error);
    }
    if let Err(error) = workspace.revalidate() {
        cleanup_created_container(backend, &created.id, &request.execution_id).await;
        return Err(error);
    }
    match tokio::time::timeout_at(deadline, backend.docker.start_container(&created.id, None)).await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            cleanup_created_container(backend, &created.id, &request.execution_id).await;
            return Err(engine_operation_error("start container", error));
        }
        Err(_) => {
            cleanup_created_container(backend, &created.id, &request.execution_id).await;
            return Err(deadline_error());
        }
    }
    match tokio::time::timeout_at(
        deadline,
        cache::install_dependency_caches(backend, &created.id, request),
    )
    .await
    {
        Ok(true) => {}
        result => {
            cleanup_created_container(backend, &created.id, &request.execution_id).await;
            return if result.is_err() {
                Err(deadline_error())
            } else {
                Err(crate::execution::docker::backend::invalid_request())
            };
        }
    }
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
    match upload {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            cleanup_created_container(backend, &created.id, &request.execution_id).await;
            return Err(error);
        }
        Err(_) => {
            cleanup_created_container(backend, &created.id, &request.execution_id).await;
            return Err(deadline_error());
        }
    }
    if let Err(error) = execution.push(ExecutionEventKindV1::Lifecycle {
        state: ExecutionLifecycleV1::Running,
    }) {
        cleanup_created_container(backend, &created.id, &request.execution_id).await;
        return Err(error);
    }
    let handle = ExecutionHandleV1 {
        execution_id: request.execution_id.clone(),
        task_id: request.task_id.clone(),
        target: request.target.clone(),
        request_sequence: request.request_sequence,
    };
    backend
        .executions
        .lock()
        .map_err(|_| internal_error_with_context("updating the execution registry"))?
        .insert(
            handle.execution_id.clone(),
            Arc::new(std::sync::Mutex::new(execution)),
        );
    reservation.activate(&handle.execution_id)?;
    Ok(handle)
}

struct WorkspaceStartReservation {
    prepared: Arc<std::sync::Mutex<super::super::super::PreparedWorkspace>>,
    active: bool,
}

impl WorkspaceStartReservation {
    fn new(prepared: Arc<std::sync::Mutex<super::super::super::PreparedWorkspace>>) -> Self {
        Self {
            prepared,
            active: true,
        }
    }

    fn activate(&mut self, execution_id: &str) -> Result<(), ExecutionError> {
        let mut prepared = self
            .prepared
            .lock()
            .map_err(|_| internal_error_with_context("activating the workspace execution"))?;
        prepared.starting = false;
        prepared.active_execution_id = Some(execution_id.to_owned());
        self.active = false;
        Ok(())
    }
}

impl Drop for WorkspaceStartReservation {
    fn drop(&mut self) {
        if self.active
            && let Ok(mut prepared) = self.prepared.lock()
        {
            prepared.starting = false;
        }
    }
}
