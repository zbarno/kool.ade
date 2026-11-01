use std::path::{Path, PathBuf};

use crate::{
    app::welcome::github::{clone_destination, parse_github_url},
    core::managed_workspace::{CodeRepositoryRegistration, ManagedWorkspace, WorkspaceManager},
};

pub(super) fn reject_unassociated_clone_destination(repo: &str) -> anyhow::Result<()> {
    let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) else {
        return Ok(());
    };
    let destination = clone_destination(repo, &PathBuf::from(home));
    match std::fs::symlink_metadata(&destination) {
        Ok(_) => anyhow::bail!(
            "The managed code clone destination {} already exists without a saved Kool.ad/e workspace. Kool.ad/e will not adopt an existing checkout; move it aside and retry.",
            destination.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => anyhow::bail!(
            "Could not inspect the managed code clone destination {}: {error}",
            destination.display()
        ),
    }
}

pub(super) fn reusable_pending_clone(
    manager: &WorkspaceManager,
    workspace_id: uuid::Uuid,
    code_remote: &str,
    planning_remote: &str,
) -> anyhow::Result<ManagedWorkspace> {
    let workspace = manager.open(workspace_id)?;
    let registered_code = workspace
        .repositories
        .repositories
        .iter()
        .find(|repository| repository.id == crate::core::managed_workspace::PAIR_CODE_REPO_ID);
    anyhow::ensure!(
        registered_code
            .is_none_or(|repository| same_github_remote(&repository.remote, code_remote)),
        "The pending clone workspace already uses its primary code slot for another repository. Kool.ad/e left it untouched."
    );
    let existing_planning_remote = manager.planning_remote_url(workspace.id)?;
    anyhow::ensure!(
        existing_planning_remote
            .as_deref()
            .is_none_or(|remote| same_github_remote(remote, planning_remote)),
        "The pending clone workspace already uses a different planning repository. Kool.ad/e left it untouched."
    );
    Ok(workspace)
}

pub(super) fn missing_planning_workspace(
    manager: &WorkspaceManager,
    reusable: Option<ManagedWorkspace>,
    title: &str,
) -> anyhow::Result<ManagedWorkspace> {
    match reusable {
        Some(workspace) => Ok(workspace),
        None => manager.create(title.trim()).map_err(|error| {
            anyhow::anyhow!("planning workspace could not be initialized: {error:#}")
        }),
    }
}

pub(super) fn reusable_local_pair(
    manager: &WorkspaceManager,
    code_remote: &str,
) -> anyhow::Result<Option<ManagedWorkspace>> {
    for local in manager.list_launcher()?.available {
        let workspace = manager.open(local.id)?;
        if workspace
            .repositories
            .repositories
            .iter()
            .any(|repository| same_github_remote(&repository.remote, code_remote))
        {
            let planning_remote = manager.planning_remote_url(workspace.id)?;
            anyhow::ensure!(
                planning_remote.is_none(),
                "A local workspace already registers this code repository with a different planning remote. Open that workspace and resolve the existing pairing before retrying."
            );
            return Ok(Some(workspace));
        }
    }
    Ok(None)
}

pub(super) fn managed_code_checkout(
    manager: &WorkspaceManager,
    workspace: &ManagedWorkspace,
    code_remote: &str,
) -> anyhow::Result<Option<PathBuf>> {
    let Some(repository) = workspace
        .repositories
        .repositories
        .iter()
        .find(|repository| same_github_remote(&repository.remote, code_remote))
    else {
        return Ok(None);
    };
    manager.checkout_path(workspace.id, &repository.id, &repository.remote)
}

pub(super) fn register_code_checkout(
    manager: &WorkspaceManager,
    workspace: ManagedWorkspace,
    checkout: &Path,
    requested_remote: &str,
) -> anyhow::Result<ManagedWorkspace> {
    let actual_remote = crate::core::gitops::origin_url(checkout)
        .ok_or_else(|| anyhow::anyhow!("the cloned code repository has no origin remote"))?;
    anyhow::ensure!(
        same_github_remote(&actual_remote, requested_remote),
        "the managed code checkout origin does not match the requested GitHub repository"
    );
    manager.register_code_repository(
        workspace.id,
        CodeRepositoryRegistration {
            id: crate::core::managed_workspace::PAIR_CODE_REPO_ID.to_owned(),
            role: "Primary code".to_owned(),
            remote: Some(actual_remote),
            checkout: checkout.to_path_buf(),
        },
    )
}

pub(super) fn same_github_remote(left: &str, right: &str) -> bool {
    fn normalize(remote: &str) -> Option<String> {
        let target = parse_github_url(remote).ok()?;
        Some(format!(
            "{}/{}",
            target.owner.to_ascii_lowercase(),
            target.repo.to_ascii_lowercase()
        ))
    }
    normalize(left).is_some() && normalize(left) == normalize(right)
}

pub(super) fn validate_planning_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        name.len() <= 100,
        "The derived planning repository name exceeds GitHub's 100-character limit. Rename the code repository before cloning it."
    );
    Ok(())
}
