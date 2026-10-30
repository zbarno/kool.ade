use std::path::Path;

use crate::{
    artifacts::planning_store::paths,
    core::project_repos::{ProjectManifest, Repository, portable_remote},
};

use super::{CodeRepositoryRegistration, ManagedWorkspace, WorkspaceManager, git};

pub(super) fn register(
    manager: &WorkspaceManager,
    workspace: &mut ManagedWorkspace,
    registration: CodeRepositoryRegistration,
) -> anyhow::Result<()> {
    let mut checkpoint = |planning_repo: &Path, message: &str, paths: &[String]| {
        crate::core::gitops::commit(planning_repo, message, paths)
            .map(|_| ())
            .map_err(anyhow::Error::from)
    };
    register_with_checkpoint(manager, workspace, registration, &mut checkpoint)
}

pub(super) fn register_with_checkpoint(
    manager: &WorkspaceManager,
    workspace: &mut ManagedWorkspace,
    registration: CodeRepositoryRegistration,
    checkpoint: &mut impl FnMut(&Path, &str, &[String]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let checkout = registration
        .checkout
        .canonicalize()
        .map_err(|error| anyhow::anyhow!("code checkout path is unreadable: {error}"))?;
    anyhow::ensure!(
        checkout.is_dir() && git::is_work_tree(&checkout),
        "code checkout is not a Git working tree"
    );
    anyhow::ensure!(
        !checkout.starts_with(&workspace.planning_repo)
            && !workspace.planning_repo.starts_with(&checkout),
        "planning and code repositories must be separate and non-nested"
    );
    let remote = registration.remote.unwrap_or_default().trim().to_owned();
    if !remote.is_empty() {
        anyhow::ensure!(
            portable_remote(&remote),
            "repository remote must be a portable identity"
        );
        anyhow::ensure!(
            git::origin(&checkout).as_deref() == Some(remote.as_str()),
            "checkout origin does not match the requested repository remote"
        );
    }

    let mut updated = ProjectManifest::load(&workspace.store)?;
    if let Some(existing) = updated
        .repositories
        .iter()
        .find(|repository| repository.id == registration.id)
    {
        anyhow::ensure!(
            existing.remote == remote,
            "repository ID {} is already registered with a different remote",
            registration.id
        );
    } else {
        anyhow::ensure!(
            updated
                .repositories
                .iter()
                .all(|repository| { remote.is_empty() || repository.remote != remote }),
            "repository remote is already registered under another ID"
        );
        updated.repositories.push(Repository {
            id: registration.id.clone(),
            role: registration.role,
            remote,
            display_name: None,
        });
    }
    updated.normalize_display_names()?;
    updated.validate()?;

    if updated != workspace.repositories {
        persist_registry(workspace, &updated, checkpoint)?;
    } else {
        let revision = workspace.store.revision()?;
        checkpoint_registry(workspace, &revision, checkpoint)?;
    }
    set_local_checkout(manager, workspace, &registration.id, &checkout)
}

fn persist_registry(
    workspace: &ManagedWorkspace,
    updated: &ProjectManifest,
    checkpoint: &mut impl FnMut(&Path, &str, &[String]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let relative = paths::PROJECT_MANIFEST;
    let bytes = serde_json::to_vec_pretty(updated)?;
    let revision = workspace.store.revision()?;
    let (_, updated_revision) = workspace
        .store
        .transaction_with_revision(&[(relative.to_owned(), bytes)], Some(&revision))?;
    checkpoint_registry(workspace, &updated_revision, checkpoint)
}

fn checkpoint_registry(
    workspace: &ManagedWorkspace,
    revision: &str,
    checkpoint: &mut impl FnMut(&Path, &str, &[String]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let relative = paths::PROJECT_MANIFEST.to_owned();
    checkpoint(
        &workspace.planning_repo,
        "Kool.ad/e: register code repository",
        std::slice::from_ref(&relative),
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "repository registry was updated but its planning checkpoint failed: {error}"
        )
    })?;
    anyhow::ensure!(
        workspace.store.revision()? == revision,
        "planning repository changed while committing the code repository registry"
    );
    Ok(())
}

fn set_local_checkout(
    manager: &WorkspaceManager,
    workspace: &mut ManagedWorkspace,
    repository_id: &str,
    checkout: &Path,
) -> anyhow::Result<()> {
    let mapping = super::CodeCheckout {
        repository_id: repository_id.to_owned(),
        path: checkout.to_path_buf(),
    };
    match workspace
        .association
        .code_checkouts
        .iter_mut()
        .find(|entry| entry.repository_id == repository_id)
    {
        Some(entry) => *entry = mapping,
        None => workspace.association.code_checkouts.push(mapping),
    }
    workspace
        .association
        .write(&manager.association_path(workspace.id))?;
    Ok(())
}
