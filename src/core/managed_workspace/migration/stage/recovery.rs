use std::{fs, path::Path};

use super::super::{RollbackOutcome, WorkspaceManager, canonical_code_checkout, journal};

pub(crate) fn workspace_for_checkout(
    manager: &WorkspaceManager,
    code_checkout: &Path,
) -> anyhow::Result<Option<super::super::super::ManagedWorkspace>> {
    let canonical = match code_checkout.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let projects = manager.home.join("projects");
    let entries = match fs::read_dir(&projects) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Ok(id) = uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        let path = manager.association_path(id);
        if !path.is_file() {
            continue;
        }
        let association = super::super::super::WorkspaceAssociation::read(&path, id)?;
        if association.code_checkouts.iter().any(|checkout| {
            checkout
                .path
                .canonicalize()
                .is_ok_and(|path| path == canonical)
        }) {
            return super::super::super::WorkspaceManager::open_unlocked(manager, id).map(Some);
        }
    }
    Ok(None)
}

pub(crate) fn rollback(
    manager: &WorkspaceManager,
    code_checkout: &Path,
) -> anyhow::Result<RollbackOutcome> {
    let code_root = canonical_code_checkout(code_checkout)?;
    let _global =
        super::super::super::locking::lock(&manager.home.join("projects/.workspace-migrate.lock"))?;
    let Some(mut workspace) = workspace_for_checkout(manager, &code_root)? else {
        anyhow::bail!("this code checkout has no managed planning association to roll back");
    };
    let _workspace_lock = super::super::super::locking::lock(
        &manager.project_dir(workspace.id).join(".workspace.lock"),
    )?;
    let journal_path = journal::path(manager, &code_root);
    let journal: journal::Journal = serde_json::from_slice(&fs::read(&journal_path)?)?;
    let current = super::verification::git_text(&workspace.planning_repo, &["rev-parse", "HEAD"])?;
    let dirty = !super::super::super::git::status_paths(&workspace.planning_repo)?.is_empty();
    workspace
        .association
        .code_checkouts
        .retain(|entry| match entry.path.canonicalize() {
            Ok(path) => path != code_root,
            Err(_) => true,
        });
    workspace
        .association
        .write(&manager.association_path(workspace.id))?;
    let warning = (dirty || journal.initial_commit.as_deref() != Some(current.as_str())).then(|| {
        "Planning changes were made after migration. Export them before relying on the legacy embedded copy."
            .to_owned()
    });
    Ok(RollbackOutcome {
        detached: true,
        warning,
    })
}
