use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::TryStreamExt;
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
) -> Result<(), ExecutionError> {
    let exec = backend
        .docker
        .create_exec(
            container_id,
            CreateExecOptions::<String> {
                attach_stdin: Some(true),
                cmd: Some(vec![
                    "tar".into(),
                    "--extract".into(),
                    "--file=-".into(),
                    "--directory".into(),
                    destination.into(),
                    "--no-same-owner".into(),
                ]),
                user: Some("10001:10001".into()),
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
        .write_all(archive)
        .await
        .map_err(|_| archive_error())?;
    input.shutdown().await.map_err(|_| archive_error())?;
    while output
        .try_next()
        .await
        .map_err(|_| archive_error())?
        .is_some()
    {}

    let result = backend
        .docker
        .inspect_exec(&exec.id)
        .await
        .map_err(|_| archive_error())?;
    if result.exit_code != Some(0) {
        return Err(archive_error());
    }
    Ok(())
}

fn archive_error() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        "Docker Engine could not materialize the isolated workspace",
        true,
    )
}
