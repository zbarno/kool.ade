use std::time::Duration;

use bollard::{
    container::LogOutput,
    models::ContainerCreateBody,
    query_parameters::{
        LogsOptionsBuilder, RemoveContainerOptionsBuilder, WaitContainerOptionsBuilder,
    },
};
use futures_util::StreamExt;

use super::super::{QaContainerUpload, denied};
use crate::execution::errors::ExecutionError;

const MAX_DRIVER_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriverContainerOutput {
    pub exit_code: i64,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Run one ephemeral app-owned driver container. Every exit path attempts
/// forced cleanup, including timeout and operator cancellation.
pub async fn run_ephemeral_driver(
    docker: &bollard::Docker,
    container_name: &str,
    body: ContainerCreateBody,
    uploads: &[QaContainerUpload],
    timeout: Duration,
    max_output_bytes: usize,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<DriverContainerOutput, ExecutionError> {
    let id = create_driver_container(
        docker,
        container_name,
        body,
        uploads,
        timeout,
        max_output_bytes,
        cancel,
    )
    .await?;
    let output = run_created_driver(docker, &id, timeout, max_output_bytes, cancel).await;
    let cleanup = cleanup_driver_container(docker, &id).await;
    match (output, cleanup) {
        (Ok(output), Ok(())) => Ok(output),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

/// Create and upload an unstarted driver. The caller can bind the returned
/// inspected container ID into the immutable run before allowing it to start.
pub async fn create_driver_container(
    docker: &bollard::Docker,
    container_name: &str,
    body: ContainerCreateBody,
    uploads: &[QaContainerUpload],
    timeout: Duration,
    max_output_bytes: usize,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<String, ExecutionError> {
    let scoped = container_name
        .strip_prefix("koolade-qa-")
        .unwrap_or_default();
    if !uuid_is_canonical(scoped)
        || timeout.is_zero()
        || timeout > Duration::from_secs(7200)
        || max_output_bytes == 0
        || max_output_bytes > MAX_DRIVER_OUTPUT_BYTES
        || *cancel.borrow()
    {
        return Err(denied(
            "QA driver launch identity, bounds, or cancellation state is invalid",
        ));
    }
    let created = docker
        .create_container(
            Some(
                bollard::query_parameters::CreateContainerOptionsBuilder::default()
                    .name(container_name)
                    .build(),
            ),
            body,
        )
        .await
        .map_err(|_| denied("QA driver container could not be created"))?;
    if created.id.is_empty() {
        return Err(denied("QA driver container could not be created"));
    }
    let container_id = created.id;
    if let Err(error) = upload_snapshots(docker, &container_id, uploads).await {
        let _ = cleanup_driver_container(docker, &container_id).await;
        return Err(error);
    }
    Ok(container_id)
}

/// Start a previously created driver and collect its bounded output. The
/// caller must always invoke cleanup, including after timeout/cancellation.
pub async fn run_created_driver(
    docker: &bollard::Docker,
    container_id: &str,
    timeout: Duration,
    max_output_bytes: usize,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<DriverContainerOutput, ExecutionError> {
    if !is_container_id(container_id)
        || timeout.is_zero()
        || timeout > Duration::from_secs(7200)
        || max_output_bytes == 0
        || max_output_bytes > MAX_DRIVER_OUTPUT_BYTES
    {
        return Err(denied("QA driver run identity or bounds are invalid"));
    }
    let exit_code = run_until_exit(docker, container_id, timeout, cancel).await?;
    read_output(docker, container_id, exit_code, max_output_bytes).await
}

pub async fn cleanup_driver_container(
    docker: &bollard::Docker,
    container_id: &str,
) -> Result<(), ExecutionError> {
    if !is_container_id(container_id) {
        return Err(denied("QA driver cleanup identity is invalid"));
    }
    docker
        .remove_container(
            container_id,
            Some(
                RemoveContainerOptionsBuilder::default()
                    .force(true)
                    .v(false)
                    .build(),
            ),
        )
        .await
        .map_err(|_| denied("QA driver container cleanup could not be confirmed"))
}

/// Remove a previous attempt only when its deterministic name and request
/// ownership labels match this exact task-scoped driver identity.
pub async fn recover_driver_container(
    docker: &bollard::Docker,
    name: &str,
    request: &crate::runtime_qa::QaRunRequestV1,
    owner_id: &str,
    driver: &str,
) -> Result<(), ExecutionError> {
    if name != format!("koolade-qa-{}", request.qa_run_id)
        || owner_id != request.execution_session_id
        || !matches!(driver, "browser" | "service" | "desktop")
    {
        return Err(denied("QA driver recovery identity is invalid"));
    }
    let Ok(inspect) = docker.inspect_container(name, None).await else {
        return Ok(());
    };
    let actual = inspect
        .config
        .and_then(|config| config.labels)
        .unwrap_or_default();
    let expected = [
        ("com.koolade.qa.owner", owner_id),
        ("com.koolade.qa.run", request.qa_run_id.as_str()),
        ("com.koolade.qa.task", request.parent_task_uid.as_str()),
        ("com.koolade.qa.driver", driver),
    ];
    if expected
        .iter()
        .any(|(key, value)| actual.get(*key).map(String::as_str) != Some(*value))
    {
        return Ok(());
    }
    let id = inspect
        .id
        .as_deref()
        .ok_or_else(|| denied("owned orphan QA driver has no inspected container ID"))?;
    cleanup_driver_container(docker, id).await
}

async fn upload_snapshots(
    docker: &bollard::Docker,
    container_id: &str,
    uploads: &[QaContainerUpload],
) -> Result<(), ExecutionError> {
    if uploads.is_empty() || uploads.len() > 4 {
        return Err(denied("QA driver upload set is empty or exceeds its bound"));
    }
    for upload in uploads {
        if !matches!(
            upload.target.as_str(),
            "/workspace" | "/run/koolade-runtime-qa"
        ) || upload.archive.is_empty()
        {
            return Err(denied("QA driver upload target or archive is invalid"));
        }
        docker
            .upload_to_container(
                container_id,
                Some(
                    bollard::query_parameters::UploadToContainerOptionsBuilder::default()
                        .path(&upload.target)
                        .copy_uidgid("1")
                        .build(),
                ),
                bollard::body_full(upload.archive.clone().into()),
            )
            .await
            .map_err(|_| denied("QA driver artifact snapshot could not be uploaded"))?;
    }
    Ok(())
}

async fn run_until_exit(
    docker: &bollard::Docker,
    container_id: &str,
    timeout: Duration,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<i64, ExecutionError> {
    docker
        .start_container(container_id, None)
        .await
        .map_err(|_| denied("QA driver container could not be started"))?;
    let mut wait = docker.wait_container(
        container_id,
        Some(
            WaitContainerOptionsBuilder::default()
                .condition("not-running")
                .build(),
        ),
    );
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return Err(denied("QA driver exceeded its approved timeout")),
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Err(denied("QA driver was cancelled"));
                }
            }
            status = wait.next() => return match status {
                Some(Ok(status)) if status.status_code == 0 => Ok(status.status_code),
                Some(Ok(_)) => Err(denied("QA driver container exited with a failure status")),
                _ => Err(denied("QA driver container ended without an exit status")),
            }
        }
    }
}

async fn read_output(
    docker: &bollard::Docker,
    container_id: &str,
    exit_code: i64,
    max_output_bytes: usize,
) -> Result<DriverContainerOutput, ExecutionError> {
    let options = LogsOptionsBuilder::default()
        .stdout(true)
        .stderr(true)
        .follow(false)
        .timestamps(false)
        .build();
    let mut logs = docker.logs(container_id, Some(options));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    while let Some(output) = logs.next().await {
        let output = output.map_err(|_| denied("QA driver logs could not be read"))?;
        match output {
            LogOutput::StdOut { message } => stdout.extend_from_slice(&message),
            LogOutput::StdErr { message } => stderr.extend_from_slice(&message),
            LogOutput::Console { message } => stdout.extend_from_slice(&message),
            LogOutput::StdIn { .. } => {}
        }
        if stdout.len().saturating_add(stderr.len()) > max_output_bytes {
            return Err(denied("QA driver output exceeds its byte limit"));
        }
    }
    Ok(DriverContainerOutput {
        exit_code,
        stdout,
        stderr,
    })
}

fn uuid_is_canonical(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn is_container_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
