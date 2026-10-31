use crate::{
    artifacts::planning_store::PlanningStore,
    artifacts::task_docs::TaskDocument,
    core::task_claim::{ClaimError, ClaimRequest, ClaimScope},
};
use std::path::Path;

pub(super) fn prepare(
    planning_store: &PlanningStore,
    code_repo: &Path,
    code_repository_id: &str,
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
    let output = crate::core::planning_sync::validated_git_command(code_repo)
        .map_err(|error| ClaimError::CoordinationRejected(error.to_string()))?
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|error| ClaimError::RemoteUnavailable(error.to_string()))?;
    let base_commit = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        let worktree = crate::core::planning_sync::validated_git_command(code_repo)
            .map_err(|error| ClaimError::CoordinationRejected(error.to_string()))?
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .map_err(|error| ClaimError::CoordinationRejected(error.to_string()))?;
        if worktree.status.success() && worktree.stdout.trim_ascii() == b"true" {
            // Git's all-zero object ID represents an unborn base while keeping
            // the shared claim payload in the same SHA format as committed bases.
            "0000000000000000000000000000000000000000".to_owned()
        } else {
            return Err(ClaimError::CoordinationRejected(
                "Cannot determine the base commit for a cross-clone task claim".into(),
            ));
        }
    };
    let scope = match planning_store.mode {
        crate::artifacts::planning_store::StoreMode::ManagedShared => ClaimScope::SharedRequired,
        crate::artifacts::planning_store::StoreMode::ManagedLocal => ClaimScope::LocalOnly,
    };
    Ok(ClaimRequest::new(
        planning_store.git_root(),
        code_repo.to_owned(),
        planning_store.project_id,
        task_uid,
        code_repository_id.to_owned(),
        base_commit,
        stale_session.map(str::to_owned),
        scope,
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
