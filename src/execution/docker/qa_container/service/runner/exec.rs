use std::time::Duration;

use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::TryStreamExt;
use tokio::io::AsyncWriteExt;

use super::DriverContainerOutput;
use crate::execution::docker::qa_container::denied;
use crate::execution::errors::ExecutionError;

pub(super) async fn run_driver_exec(
    docker: &bollard::Docker,
    container_id: &str,
    command: Vec<String>,
    timeout: Duration,
    max_output_bytes: usize,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<DriverContainerOutput, ExecutionError> {
    let output = run_attached_exec(docker, container_id, command, None, max_output_bytes);
    let deadline = tokio::time::Instant::now() + timeout;
    tokio::select! {
        _ = tokio::time::sleep_until(deadline) => Err(denied("QA driver exceeded its approved timeout")),
        changed = cancel.changed() => {
            let _ = changed;
            Err(denied("QA driver was cancelled"))
        }
        result = output => result,
    }
}

pub(super) async fn run_attached_exec(
    docker: &bollard::Docker,
    container_id: &str,
    command: Vec<String>,
    input_bytes: Option<&[u8]>,
    max_output_bytes: usize,
) -> Result<DriverContainerOutput, ExecutionError> {
    let exec = docker
        .create_exec(
            container_id,
            CreateExecOptions::<String> {
                attach_stdin: Some(input_bytes.is_some()),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(command),
                user: Some("10001:10001".into()),
                privileged: Some(false),
                ..Default::default()
            },
        )
        .await
        .map_err(|_| denied("QA driver command could not be created"))?;
    let StartExecResults::Attached {
        mut input,
        mut output,
    } = docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                output_capacity: Some(64 * 1024),
            }),
        )
        .await
        .map_err(|_| denied("QA driver command could not be started"))?
    else {
        return Err(denied("QA driver command did not attach"));
    };
    if let Some(bytes) = input_bytes {
        input
            .write_all(bytes)
            .await
            .map_err(|_| denied("QA driver artifact snapshot could not be streamed"))?;
        input
            .shutdown()
            .await
            .map_err(|_| denied("QA driver command input could not be closed"))?;
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    while let Some(output) = output
        .try_next()
        .await
        .map_err(|_| denied("QA driver output stream could not be read"))?
    {
        match output {
            LogOutput::StdOut { message } | LogOutput::Console { message } => {
                stdout.extend_from_slice(&message)
            }
            LogOutput::StdErr { message } => stderr.extend_from_slice(&message),
            LogOutput::StdIn { .. } => continue,
        }
        if stdout.len().saturating_add(stderr.len()) > max_output_bytes {
            return Err(denied("QA driver output exceeded its approved byte limit"));
        }
    }
    let status = docker
        .inspect_exec(&exec.id)
        .await
        .map_err(|_| denied("QA driver command result could not be inspected"))?;
    let exit_code = status
        .exit_code
        .ok_or_else(|| denied("QA driver command ended without an exit status"))?;
    Ok(DriverContainerOutput {
        exit_code,
        stdout,
        stderr,
    })
}
