use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

use crate::execution::{
    contract::ExecutionRequestV1,
    docker::{
        backend::DockerExecutionBackend,
        sandbox::{ValidatedWorkspace, WorkspaceSnapshot, staging},
    },
    errors::{ExecutionError, ExecutionErrorKind},
};

mod cancellation;
pub(super) use cancellation::{WorkspaceSyncRequest, sync_workspace_if_active};

pub(in crate::execution::docker::backend::lifecycle) async fn sync_workspace(
    backend: &DockerExecutionBackend,
    container_id: &str,
    workspace: &ValidatedWorkspace,
    snapshot: Option<&WorkspaceSnapshot>,
    budget: u64,
    writable: bool,
    authorization_generation: &str,
) -> Result<(), ExecutionError> {
    if !writable {
        return Ok(());
    }
    backend.verify_authorization(authorization_generation)?;
    let Some(snapshot) = snapshot else {
        return Err(ExecutionError::new(
            ExecutionErrorKind::WorkerDisconnected,
            "Workspace baseline is unavailable after recovery; task clone was preserved",
            false,
        ));
    };
    let archive = download_workspace(backend, container_id, budget).await?;
    backend.verify_authorization(authorization_generation)?;
    staging::synchronize(workspace, snapshot, &archive, budget)
}

pub(in crate::execution::docker::backend::lifecycle) fn workspace_write_enabled(
    request: &ExecutionRequestV1,
) -> bool {
    request.mode != crate::execution::contract::ExecutionModeV1::PlanningReadOnly
        && request.policy.filesystem_grants().iter().any(|grant| {
            matches!(
                grant,
                crate::execution::policy::FilesystemGrantV1::WorkspaceWrite
                    | crate::execution::policy::FilesystemGrantV1::WorkspaceScopedWrite
            )
        })
}

pub(super) async fn download_workspace(
    backend: &DockerExecutionBackend,
    container_id: &str,
    budget: u64,
) -> Result<Vec<u8>, ExecutionError> {
    let limit = budget.min(crate::execution::docker::sandbox::staging::MAX_STAGED_BYTES);
    let exec = match backend
        .docker
        .create_exec(
            container_id,
            CreateExecOptions::<String> {
                attach_stdin: Some(false),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(vec![
                    "tar".into(),
                    "--create".into(),
                    "--file=-".into(),
                    "--directory=/workspace".into(),
                    "--exclude=./.git".into(),
                    ".".into(),
                ]),
                user: Some("10001:10001".into()),
                privileged: Some(false),
                ..Default::default()
            },
        )
        .await
    {
        Ok(exec) => exec,
        Err(error) => {
            return Err(super::state::operation_disconnected(
                "create the workspace archive command",
                &error,
            ));
        }
    };
    let started = match backend
        .docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                output_capacity: Some(64 * 1024),
            }),
        )
        .await
    {
        Ok(started) => started,
        Err(error) => {
            return Err(super::state::operation_disconnected(
                "start the workspace archive command",
                &error,
            ));
        }
    };
    let StartExecResults::Attached {
        mut output,
        mut input,
    } = started
    else {
        return Err(ExecutionError::new(
            ExecutionErrorKind::WorkerDisconnected,
            "Docker did not attach the workspace archive command",
            true,
        ));
    };
    input.shutdown().await.map_err(|error| {
        let detail = if error.kind() == std::io::ErrorKind::BrokenPipe {
            "Docker closed the workspace archive input"
        } else {
            "Docker could not close the workspace archive input"
        };
        ExecutionError::new(ExecutionErrorKind::WorkerDisconnected, detail, true)
    })?;
    let mut archive = Vec::new();
    let mut stderr_tail = Vec::new();
    while let Some(output) = output.next().await {
        let output = output.map_err(|error| {
            super::state::operation_disconnected("read the workspace archive", &error)
        })?;
        match output {
            LogOutput::StdOut { message } => {
                if (archive.len() as u64).saturating_add(message.len() as u64) > limit {
                    return Err(ExecutionError::new(
                        ExecutionErrorKind::ExecutionFailed,
                        "Container workspace archive exceeded its synchronization limit",
                        false,
                    ));
                }
                archive.extend_from_slice(&message);
            }
            LogOutput::StdErr { message } => {
                stderr_tail.extend_from_slice(&message);
                if stderr_tail.len() > 512 {
                    stderr_tail.drain(..stderr_tail.len() - 512);
                }
            }
            _ => {}
        }
    }
    let inspection = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = match backend.docker.inspect_exec(&exec.id).await {
                Ok(status) => status,
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404, ..
                }) => return Ok::<_, ExecutionError>(None),
                Err(error) => {
                    return Err(super::state::operation_disconnected(
                        "inspect the workspace archive command",
                        &error,
                    ));
                }
            };
            if status.running == Some(false) || status.exit_code.is_some() {
                return Ok(Some(status.exit_code));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| {
        ExecutionError::new(
            ExecutionErrorKind::WorkerDisconnected,
            "Docker workspace archive command exceeded its bounded deadline",
            true,
        )
    })??;
    let Some(exit_code) = inspection else {
        // Docker's archive API cannot see the live tmpfs. Never apply an
        // empty stopped-container archive as deletions to the task clone.
        return Err(super::state::disconnected());
    };
    if exit_code != Some(0) {
        let detail = stderr_summary(&stderr_tail);
        return Err(ExecutionError::new(
            ExecutionErrorKind::WorkerDisconnected,
            format!("Docker workspace archive command failed: {detail}"),
            true,
        ));
    }
    Ok(archive)
}

fn stderr_summary(stderr: &[u8]) -> &'static str {
    let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if stderr.contains("permission denied") {
        "the archive process lacked file permissions"
    } else if stderr.contains("no space left") {
        "container storage is full"
    } else if stderr.trim().is_empty() {
        "the archive process produced no diagnostic output"
    } else {
        "the archive tool reported an error"
    }
}

#[cfg(test)]
mod tests {
    use super::stderr_summary;

    #[test]
    fn archive_diagnostic_classifies_permission_failures_without_echoing_paths() {
        let summary = stderr_summary(b"tar: /workspace/customer-name/file: Permission denied");

        assert_eq!(summary, "the archive process lacked file permissions");
        assert!(!summary.contains("customer-name"));
    }

    #[test]
    fn archive_diagnostic_classifies_empty_and_generic_failures() {
        assert_eq!(
            stderr_summary(b""),
            "the archive process produced no diagnostic output"
        );
        assert_eq!(
            stderr_summary(b"tar: /workspace/private-name/file: unsupported type"),
            "the archive tool reported an error"
        );
    }
}
