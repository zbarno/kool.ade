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
            staging_user(target),
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
            staging_user(target),
            &upload.archive,
            deadline,
            cancel,
        )
        .await?;
        if staged.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&staged.stderr);
            let message = if stderr.contains("Permission denied") {
                match target {
                    "/workspace" => {
                        "QA workspace artifact staging was denied by target permissions"
                    }
                    "/home/executor/.npm" => {
                        "QA npm cache staging was denied by target permissions"
                    }
                    "/home/executor/.cargo" => {
                        "QA Cargo cache staging was denied by target permissions"
                    }
                    "/home/executor/.cache/koolade/pip" => {
                        "QA Python cache staging was denied by target permissions"
                    }
                    "/home/executor/go" => "QA Go cache staging was denied by target permissions",
                    _ => "QA runtime request staging was denied by target permissions",
                }
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
    if uploads.iter().any(|upload| upload.target == "/workspace") {
        let locked = run_staging_command(
            docker,
            container_id,
            vec![
                "chmod".into(),
                "-R".into(),
                "a-w".into(),
                "/workspace".into(),
            ],
            "0:0".into(),
            &[],
            deadline,
            cancel,
        )
        .await?;
        if locked.exit_code != 0 {
            return Err(denied(
                "QA workspace artifact permissions could not be restored after staging",
            ));
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

fn staging_user(target: &str) -> String {
    if target == "/workspace" {
        "0:0".into()
    } else {
        format!(
            "{}:{}",
            crate::execution::docker::images::EXECUTOR_UID,
            crate::execution::docker::images::EXECUTOR_GID
        )
    }
}

async fn run_staging_command(
    docker: &bollard::Docker,
    container_id: &str,
    command: Vec<String>,
    user: String,
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
        result = run_attached_exec(docker, container_id, command, Some(archive), user, 1024) => result,
    }
}

#[cfg(test)]
mod tests {
    use super::{approved_upload_target, staging_user};

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

    #[test]
    fn only_workspace_staging_uses_the_container_root_identity() {
        assert_eq!(staging_user("/workspace"), "0:0");
        assert_eq!(
            staging_user("/home/executor/.npm"),
            format!(
                "{}:{}",
                crate::execution::docker::images::EXECUTOR_UID,
                crate::execution::docker::images::EXECUTOR_GID
            )
        );
    }
}
