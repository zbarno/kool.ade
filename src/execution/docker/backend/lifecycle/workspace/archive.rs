use bollard::container::LogOutput;
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
        "extract the isolated workspace archive",
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
            "seal the planning workspace",
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
    action: &'static str,
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
        .map_err(|_| archive_error(action, "Docker rejected the staging command"))?;
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
        .map_err(|_| archive_error(action, "Docker could not start the staging command"))?
    else {
        return Err(archive_error(
            action,
            "Docker did not attach the staging command",
        ));
    };
    input
        .write_all(input_bytes)
        .await
        .map_err(|_| archive_error(action, "Docker closed the staging input"))?;
    input
        .shutdown()
        .await
        .map_err(|_| archive_error(action, "Docker could not close the staging input"))?;
    let mut stderr_tail = Vec::new();
    while let Some(output) = output
        .try_next()
        .await
        .map_err(|_| archive_error(action, "Docker closed the staging output"))?
    {
        if let LogOutput::StdErr { message } = output {
            stderr_tail.extend_from_slice(&message);
            if stderr_tail.len() > 512 {
                stderr_tail.drain(..stderr_tail.len() - 512);
            }
        }
    }
    loop {
        let result =
            backend.docker.inspect_exec(&exec.id).await.map_err(|_| {
                archive_error(action, "Docker could not inspect the staging command")
            })?;
        if result.running == Some(false) || result.exit_code.is_some() {
            if result.exit_code == Some(0) {
                return Ok(());
            }
            let detail = format!(
                "staging command exited with status {:?}; {}",
                result.exit_code,
                stderr_summary(&stderr_tail)
            );
            return Err(archive_error(action, &detail));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn stderr_summary(stderr: &[u8]) -> &'static str {
    let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if stderr.contains("permission denied") {
        "the staging process lacked file permissions"
    } else if stderr.contains("no space left") {
        "container storage is full"
    } else if stderr.contains("invalid tar") || stderr.contains("unexpected eof") {
        "the workspace archive is invalid or incomplete"
    } else if stderr.trim().is_empty() {
        "the staging command produced no diagnostic output"
    } else {
        "the staging tool reported an error"
    }
}

fn archive_error(action: &str, detail: &str) -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        format!("Docker Engine could not {action}: {detail}"),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::stderr_summary;

    #[test]
    fn archive_diagnostic_classifies_permission_failures_without_echoing_paths() {
        let summary = stderr_summary(b"tar: /workspace/customer-name/file: Permission denied");

        assert_eq!(summary, "the staging process lacked file permissions");
        assert!(!summary.contains("customer-name"));
    }

    #[test]
    fn archive_diagnostic_classifies_invalid_input_without_echoing_archive_names() {
        let summary = stderr_summary(b"tar: private-project.tar: Unexpected EOF");

        assert_eq!(summary, "the workspace archive is invalid or incomplete");
        assert!(!summary.contains("private-project"));
    }
}
