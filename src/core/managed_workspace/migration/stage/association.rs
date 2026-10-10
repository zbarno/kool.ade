use std::path::Path;

use super::populate::Snapshot;

mod validation;

pub(super) use validation::validate_migrated_final;

pub(super) fn association_for(
    snapshot: &Snapshot,
    workspace_id: uuid::Uuid,
    planning_repo: &Path,
) -> anyhow::Result<super::super::super::WorkspaceAssociation> {
    use super::super::super::{CodeCheckout, WorkspaceAssociation};
    let mut association = WorkspaceAssociation::new(workspace_id, planning_repo.to_path_buf());
    for checkout in snapshot
        .source_checkouts
        .iter()
        .filter(|checkout| !checkout.linked_worktree)
    {
        association.code_checkouts.push(CodeCheckout {
            repository_id: checkout.repository_id.clone(),
            path: checkout.path.clone(),
        });
    }
    if !association
        .code_checkouts
        .iter()
        .any(|entry| entry.repository_id == "root")
    {
        association.code_checkouts.push(CodeCheckout {
            repository_id: "root".into(),
            path: snapshot.code_root.clone(),
        });
    }
    Ok(association)
}
