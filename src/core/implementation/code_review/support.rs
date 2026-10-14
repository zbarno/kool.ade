use super::Runner;
use crate::harness::ReviewExecutionIdentity;
use sha2::{Digest, Sha256};
use std::path::Path;

pub(super) fn requirement_matrix(ticket: &str, task_uid: &str) -> Vec<String> {
    let checks = ticket
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.starts_with("- [ ]") || line.starts_with("- [x]") || line.starts_with("- [X]")
        })
        .map(|line| crate::core::context_build::clip(line[5..].trim(), 1000))
        .filter(|line| !line.is_empty())
        .take(64)
        .collect::<Vec<_>>();
    if checks.is_empty() {
        vec![format!("approved-task-scope:{task_uid}")]
    } else {
        checks
    }
}

pub(super) fn ensure_identity(
    identity: &ReviewExecutionIdentity,
    label: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        is_uuid(&identity.execution_id) && is_uuid(&identity.session_id),
        "{label} controller identity is invalid"
    );
    Ok(())
}
pub(super) fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}
pub(super) fn git_head(runner: &Runner, repo: &Path) -> anyhow::Result<String> {
    runner.git(repo, &["rev-parse", "HEAD"])
}
pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
