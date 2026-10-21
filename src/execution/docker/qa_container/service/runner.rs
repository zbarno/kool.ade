use std::time::Duration;

use bollard::{models::ContainerCreateBody, query_parameters::RemoveContainerOptionsBuilder};

use super::super::{QaContainerUpload, denied};
use crate::execution::errors::ExecutionError;

mod exec;
mod staging;
use exec::run_driver_exec;
use staging::upload_snapshots;

pub(crate) use exec::start_detached_exec;
pub(crate) use staging::upload_snapshots as stage_qa_uploads;

const MAX_DRIVER_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const DRIVER_LABEL: &str = "com.koolade.qa.driver";
pub(super) const DRIVER_MODE_LABEL: &str = "com.koolade.qa.mode";

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

/// Start an idle driver, stage snapshots into its tmpfs mounts, and return its
/// inspected identity for durable binding before executing the driver.
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
    let mut body = body;
    let driver = body
        .labels
        .as_ref()
        .and_then(|labels| labels.get(DRIVER_LABEL))
        .map(String::as_str)
        .unwrap_or_default();
    let requested_args = body.cmd.take().unwrap_or_default();
    let mode = match (driver, requested_args.as_slice()) {
        ("browser", []) | ("desktop", []) => "default",
        ("desktop", [arg]) if arg == "--fixture-smoke" => "fixture-smoke",
        ("desktop", [arg]) if arg == "--fixture-cache-smoke" => "fixture-cache-smoke",
        _ => return Err(denied("QA driver command is not an approved fixed mode")),
    };
    let labels = body.labels.get_or_insert_with(Default::default);
    labels.insert(DRIVER_MODE_LABEL.into(), mode.into());
    body.entrypoint = Some(vec!["/usr/bin/sleep".into()]);
    body.cmd = Some(vec!["infinity".into()]);

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
    if docker.start_container(&container_id, None).await.is_err() {
        let _ = cleanup_driver_container(docker, &container_id).await;
        return Err(denied(
            "QA driver container could not be started for staging",
        ));
    }
    if let Err(error) = upload_snapshots(docker, &container_id, uploads, timeout, cancel).await {
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
    let output =
        execute_created_driver(docker, container_id, timeout, max_output_bytes, cancel).await?;
    if output.exit_code != 0 {
        return Err(denied("QA driver command exited with a failure status"));
    }
    Ok(output)
}

#[cfg(test)]
pub(crate) async fn run_created_driver_with_status(
    docker: &bollard::Docker,
    container_id: &str,
    timeout: Duration,
    max_output_bytes: usize,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<DriverContainerOutput, ExecutionError> {
    execute_created_driver(docker, container_id, timeout, max_output_bytes, cancel).await
}

async fn execute_created_driver(
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
    let inspect = docker
        .inspect_container(container_id, None)
        .await
        .map_err(|_| denied("QA driver identity could not be inspected"))?;
    let labels = inspect
        .config
        .and_then(|config| config.labels)
        .unwrap_or_default();
    let command = match (
        labels.get(DRIVER_LABEL).map(String::as_str),
        labels.get(DRIVER_MODE_LABEL).map(String::as_str),
    ) {
        (Some("browser"), Some("default")) => {
            vec!["node".into(), "/opt/koolade-runtime-qa/driver.mjs".into()]
        }
        (Some("desktop"), Some("default")) => vec!["/usr/local/bin/koolade-qa-desktop".into()],
        (Some("desktop"), Some("fixture-smoke")) => vec![
            "/usr/local/bin/koolade-qa-desktop".into(),
            "--fixture-smoke".into(),
        ],
        (Some("desktop"), Some("fixture-cache-smoke")) => vec![
            "/usr/local/bin/koolade-qa-desktop".into(),
            "--fixture-cache-smoke".into(),
        ],
        _ => return Err(denied("QA driver launch mode is not approved")),
    };
    let output = run_driver_exec(
        docker,
        container_id,
        command,
        timeout,
        max_output_bytes,
        cancel,
    )
    .await?;
    Ok(output)
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

fn uuid_is_canonical(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn is_container_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
