use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::TryStreamExt;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

use crate::execution::{
    docker::backend::DockerExecutionBackend,
    errors::{ExecutionError, ExecutionErrorKind},
};

pub async fn extract_archive(
    backend: &DockerExecutionBackend,
    container_id: &str,
    destination: &str,
    archive: &[u8],
    seal_workspace: bool,
) -> Result<(), ExecutionError> {
    run_exec(
        backend,
        container_id,
        vec![
            "tar".into(),
            "--extract".into(),
            "--file=-".into(),
            "--directory".into(),
            destination.into(),
            "--no-same-owner".into(),
        ],
        if seal_workspace { "0:0" } else { "10001:10001" },
        archive,
    )
    .await?;
    if seal_workspace {
        run_exec(
            backend,
            container_id,
            vec![
                "/bin/sh".into(),
                "-c".into(),
                "find /workspace -path /workspace/.git -prune -o -exec chmod a+rX,a-w {} +".into(),
            ],
            "0:0",
            &[],
        )
        .await?;
    }
    Ok(())
}

async fn run_exec(
    backend: &DockerExecutionBackend,
    container_id: &str,
    command: Vec<String>,
    user: &str,
    input_bytes: &[u8],
) -> Result<(), ExecutionError> {
    let exec = backend
        .docker
        .create_exec(
            container_id,
            CreateExecOptions::<String> {
                attach_stdin: Some(true),
                attach_stdout: Some(false),
                attach_stderr: Some(false),
                cmd: Some(command),
                user: Some(user.into()),
                privileged: Some(false),
                ..Default::default()
            },
        )
        .await
        .map_err(|_| archive_error())?;
    let StartExecResults::Attached {
        mut input,
        mut output,
    } = backend
        .docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                output_capacity: Some(16 * 1024),
            }),
        )
        .await
        .map_err(|_| archive_error())?
    else {
        return Err(archive_error());
    };
    input
        .write_all(input_bytes)
        .await
        .map_err(|_| archive_error())?;
    input.shutdown().await.map_err(|_| archive_error())?;
    while output
        .try_next()
        .await
        .map_err(|_| archive_error())?
        .is_some()
    {}
    loop {
        let result = backend
            .docker
            .inspect_exec(&exec.id)
            .await
            .map_err(|_| archive_error())?;
        if result.running == Some(false) || result.exit_code.is_some() {
            if result.exit_code == Some(0) {
                return Ok(());
            }
            return Err(archive_error());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn archive_error() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        "Docker Engine could not materialize the isolated workspace",
        true,
    )
}
