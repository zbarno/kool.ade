use crate::{
    artifacts::task_docs::TaskDocument,
    core::task_claim::{ClaimError, ClaimRequest},
};
use std::path::Path;

pub(super) fn prepare(
    repo: &Path,
    documents: &[TaskDocument],
    ticket: &str,
    stale_session: Option<&str>,
    allow_offline: bool,
) -> Result<ClaimRequest, ClaimError> {
    let task_uid = documents
        .iter()
        .find(|document| document.path == ticket)
        .and_then(|document| document.identity.as_ref())
        .map(|identity| identity.uid.clone())
        .unwrap_or_else(|| legacy_path_identity(ticket));
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .map_err(|error| ClaimError::RemoteUnavailable(error.to_string()))?;
    let base_commit = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        let worktree = std::process::Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(repo)
            .output()
            .map_err(|error| ClaimError::CoordinationRejected(error.to_string()))?;
        if worktree.status.success() && worktree.stdout.trim_ascii() == b"true" {
            "unborn".to_owned()
        } else {
            return Err(ClaimError::CoordinationRejected(
                "Cannot determine the base commit for a cross-clone task claim".into(),
            ));
        }
    };
    Ok(ClaimRequest::new(
        repo.to_owned(),
        task_uid,
        base_commit,
        stale_session.map(str::to_owned),
        allow_offline,
    ))
}

/// Legacy task cards without embedded UIDs still need the same key in every
/// clone. The canonical task path is the stable fallback until migration adds
/// a persisted UID.
fn legacy_path_identity(ticket: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(ticket.as_bytes());
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("path-{hex}")
}
