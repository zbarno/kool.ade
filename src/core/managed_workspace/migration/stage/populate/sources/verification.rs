use std::{fs, path::Path};

use crate::core::managed_workspace::migration::inventory;

use super::SourceCheckout;

pub(crate) fn current_artifacts(
    checkout: &SourceCheckout,
) -> anyhow::Result<(Vec<inventory::Artifact>, Vec<inventory::BranchArtifact>)> {
    let planning_root = checkout.path.join(crate::artifacts::layout::store::ROOT);
    let exists = match fs::symlink_metadata(&planning_root) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "legacy planning root must remain a real directory: {}",
                planning_root.display()
            );
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    if !exists {
        anyhow::ensure!(
            !checkout.has_planning_root,
            "legacy planning root disappeared during migration"
        );
        return Ok((
            Vec::new(),
            inventory::collect_branch_artifacts(
                &checkout.path,
                &checkout.repository_id,
                &checkout.git_ref,
            )?,
        ));
    }
    anyhow::ensure!(
        checkout.has_planning_root,
        "legacy planning root appeared during migration"
    );
    Ok((
        inventory::collect(&checkout.path, &checkout.repository_id)?,
        inventory::collect_branch_artifacts(
            &checkout.path,
            &checkout.repository_id,
            &checkout.git_ref,
        )?,
    ))
}

pub(crate) fn current_linked_worktree_artifacts(
    checkout: &SourceCheckout,
) -> anyhow::Result<Vec<inventory::BranchArtifact>> {
    super::worktrees::current_artifacts(checkout)
}

pub(super) fn active_ref(path: &Path) -> anyhow::Result<String> {
    let output = crate::core::managed_workspace::git::output(
        Some(path),
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )?;
    Ok(if output.status.success() {
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        "HEAD".into()
    })
}

pub(super) fn git_text(root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = crate::core::managed_workspace::git::run(Some(root), args)?;
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

pub(super) fn is_not_found(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|source| source.kind() == std::io::ErrorKind::NotFound)
}
