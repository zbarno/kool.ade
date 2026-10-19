use std::time::Duration;

use super::super::super::{QaContainerUpload, denied};
use super::DriverContainerOutput;
use super::exec::run_attached_exec;
use crate::execution::errors::ExecutionError;

pub(crate) async fn upload_snapshots(
    docker: &bollard::Docker,
    container_id: &str,
    uploads: &[QaContainerUpload],
    timeout: Duration,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<(), ExecutionError> {
    if uploads.is_empty() || uploads.len() > 4 {
        return Err(denied("QA upload set is empty or exceeds its bound"));
    }
    let deadline = tokio::time::Instant::now() + timeout;
    for upload in uploads {
        if !matches!(
            upload.target.as_str(),
            "/workspace"
                | "/run/koolade-runtime-qa"
                | "/home/executor/.npm"
                | "/home/executor/.cargo"
        ) || upload.archive.is_empty()
        {
            return Err(denied("QA upload target or archive is invalid"));
        }
        let target = upload.target.as_str();
        let mkdir = run_staging_command(
            docker,
            container_id,
            vec!["mkdir".into(), "-p".into(), target.into()],
            &[],
            deadline,
            cancel,
        )
        .await?;
        if mkdir.exit_code != 0 {
            return Err(denied("QA upload target could not be prepared"));
        }
        let staged = run_staging_command(
            docker,
            container_id,
            vec![
                "tar".into(),
                "-x".into(),
                "--no-same-owner".into(),
                "--no-same-permissions".into(),
                "-f".into(),
                "-".into(),
                "-C".into(),
                target.into(),
            ],
            &upload.archive,
            deadline,
            cancel,
        )
        .await?;
        if staged.exit_code != 0 {
            return Err(denied("QA artifact snapshot could not be staged"));
        }
    }
    Ok(())
}

async fn run_staging_command(
    docker: &bollard::Docker,
    container_id: &str,
    command: Vec<String>,
    archive: &[u8],
    deadline: tokio::time::Instant,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<DriverContainerOutput, ExecutionError> {
    tokio::select! {
        _ = tokio::time::sleep_until(deadline) => Err(denied("QA artifact staging exceeded its approved timeout")),
        changed = cancel.changed() => {
            let _ = changed;
            Err(denied("QA artifact staging was cancelled"))
        }
        result = run_attached_exec(docker, container_id, command, Some(archive), 1024) => result,
    }
}
