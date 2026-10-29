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
    if uploads.is_empty() || uploads.len() > 10 {
        return Err(denied("QA upload set is empty or exceeds its bound"));
    }
    let deadline = tokio::time::Instant::now() + timeout;
    for upload in uploads {
        if !approved_upload_target(&upload.target) || upload.archive.is_empty() {
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
            let stderr = String::from_utf8_lossy(&staged.stderr);
            let message = if stderr.contains("Permission denied") {
                "QA artifact snapshot staging was denied by target permissions"
            } else if stderr.contains("No such file or directory") {
                "QA artifact snapshot staging could not find an archive path"
            } else if stderr.contains("not found") {
                "QA artifact snapshot staging utility is unavailable"
            } else {
                "QA artifact snapshot could not be staged"
            };
            return Err(denied(message));
        }
    }
    Ok(())
}

fn approved_upload_target(target: &str) -> bool {
    matches!(
        target,
        "/workspace"
            | "/run/koolade-runtime-qa"
            | "/home/executor/.npm"
            | "/home/executor/.cargo"
            | "/home/executor/.cache/koolade/pip"
            | "/home/executor/go"
    )
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

#[cfg(test)]
mod tests {
    use super::approved_upload_target;

    #[test]
    fn qa_upload_targets_are_limited_to_approved_artifact_and_dependency_cache_paths() {
        for target in [
            "/workspace",
            "/run/koolade-runtime-qa",
            "/home/executor/.npm",
            "/home/executor/.cargo",
            "/home/executor/.cache/koolade/pip",
            "/home/executor/go",
        ] {
            assert!(approved_upload_target(target), "{target}");
        }
        for target in ["/", "/home/executor", "/home/executor/.cache", "/tmp"] {
            assert!(!approved_upload_target(target), "{target}");
        }
    }
}
