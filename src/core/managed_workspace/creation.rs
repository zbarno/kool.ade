use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::{
    WorkspaceManager,
    association::WorkspaceAssociation,
    git, locking,
    manager::{ManagedWorkspace, sync_directory},
    manifest::WorkspaceManifest,
};

pub(super) mod initialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
pub(crate) enum CreationFailurePoint {
    GitInit,
    InitialCommit,
    Renamed,
    Associated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreationJournal {
    schema_version: u32,
    workspace_id: uuid::Uuid,
    title: String,
}

impl CreationJournal {
    fn validate(&self, expected_id: uuid::Uuid) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported workspace creation journal"
        );
        anyhow::ensure!(
            self.workspace_id == expected_id,
            "workspace creation journal identity mismatch"
        );
        Ok(())
    }
}

pub(super) fn create(
    manager: &WorkspaceManager,
    title: &str,
    #[cfg(test)] failure: Option<CreationFailurePoint>,
    #[cfg(not(test))] _failure: Option<()>,
) -> anyhow::Result<ManagedWorkspace> {
    validate_title(title)?;
    let projects = manager.home.join("projects");
    fs::create_dir_all(&projects)?;
    let _global_lock = locking::lock(&projects.join(".workspace-create.lock"))?;
    if let Some(pending) = recover_pending_locked(manager)? {
        return Ok(pending);
    }

    let id = uuid::Uuid::new_v4();
    let project_dir = manager.project_dir(id);
    fs::create_dir(&project_dir)?;
    let _workspace_lock = locking::lock(&project_dir.join(".workspace.lock"))?;
    write_journal(manager, id, title)?;
    finish_creation(
        manager,
        id,
        title,
        #[cfg(test)]
        failure,
    )
}

pub(super) fn recover_id(manager: &WorkspaceManager, id: uuid::Uuid) -> anyhow::Result<()> {
    let journal_path = journal_path(manager, id);
    if !journal_path.is_file() {
        return Ok(());
    }
    let _lock = locking::lock(&manager.project_dir(id).join(".workspace.lock"))?;
    let journal = read_journal(&journal_path, id)?;
    finish_creation(
        manager,
        id,
        &journal.title,
        #[cfg(test)]
        None,
    )?;
    Ok(())
}

fn recover_pending_locked(manager: &WorkspaceManager) -> anyhow::Result<Option<ManagedWorkspace>> {
    let projects = manager.home.join("projects");
    let mut pending = Vec::new();
    for entry in fs::read_dir(&projects)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let journal = entry.path().join("creation.json");
        if !journal.is_file() {
            continue;
        }
        let id = uuid::Uuid::parse_str(&entry.file_name().to_string_lossy())
            .map_err(|_| anyhow::anyhow!("pending workspace directory has an invalid UUID"))?;
        pending.push((id, journal));
    }
    anyhow::ensure!(
        pending.len() <= 1,
        "multiple incomplete workspace creations require explicit recovery"
    );
    let Some((id, path)) = pending.pop() else {
        return Ok(None);
    };
    let _lock = locking::lock(&manager.project_dir(id).join(".workspace.lock"))?;
    let journal = read_journal(&path, id)?;
    finish_creation(
        manager,
        id,
        &journal.title,
        #[cfg(test)]
        None,
    )
    .map(Some)
}

fn write_journal(manager: &WorkspaceManager, id: uuid::Uuid, title: &str) -> anyhow::Result<()> {
    let journal = CreationJournal {
        schema_version: 1,
        workspace_id: id,
        title: title.to_owned(),
    };
    crate::artifacts::atomic_write_bytes(
        &journal_path(manager, id),
        &serde_json::to_vec_pretty(&journal)?,
    )
}

fn read_journal(path: &Path, id: uuid::Uuid) -> anyhow::Result<CreationJournal> {
    let journal: CreationJournal = serde_json::from_slice(&fs::read(path)?)?;
    journal.validate(id)?;
    validate_title(&journal.title)?;
    Ok(journal)
}

fn journal_path(manager: &WorkspaceManager, id: uuid::Uuid) -> PathBuf {
    manager.project_dir(id).join("creation.json")
}

fn finish_creation(
    manager: &WorkspaceManager,
    id: uuid::Uuid,
    title: &str,
    #[cfg(test)] failure: Option<CreationFailurePoint>,
) -> anyhow::Result<ManagedWorkspace> {
    let project_dir = manager.project_dir(id);
    let stage = project_dir.join("planning-repo.staging");
    let final_repo = manager.planning_repo_path(id);
    let association_path = manager.association_path(id);

    if association_path.is_file() {
        let workspace = WorkspaceManager::open_unlocked(manager, id)?;
        remove_journal(manager, id)?;
        return Ok(workspace);
    }

    if final_repo.exists() {
        validate_final_repository(&final_repo, id)?;
        WorkspaceAssociation::new(id, final_repo.clone()).write(&association_path)?;
        #[cfg(test)]
        fail_after(CreationFailurePoint::Associated, failure)?;
        remove_journal(manager, id)?;
        return WorkspaceManager::open_unlocked(manager, id);
    }

    if !stage.exists() {
        fs::create_dir(&stage)?;
        git::init_planning_repo(&stage)?;
        #[cfg(test)]
        fail_after(CreationFailurePoint::GitInit, failure)?;
    } else {
        if git::repository_root(&stage).is_err() {
            let mut entries = fs::read_dir(&stage)?;
            anyhow::ensure!(
                entries.next().is_none(),
                "planning staging directory contains unrelated data"
            );
            git::init_planning_repo(&stage)?;
            #[cfg(test)]
            fail_after(CreationFailurePoint::GitInit, failure)?;
        } else {
            anyhow::ensure!(
                git::repository_root(&stage)? == stage,
                "planning staging directory contains an unrelated repository"
            );
        }
    }

    initialize::populate_and_commit(&stage, id, title)?;
    #[cfg(test)]
    fail_after(CreationFailurePoint::InitialCommit, failure)?;
    initialize::sync_tree(&stage)?;

    anyhow::ensure!(
        !final_repo.exists(),
        "planning repository destination already exists; it was not overwritten"
    );
    fs::rename(&stage, &final_repo)?;
    sync_directory(&project_dir)?;
    #[cfg(test)]
    fail_after(CreationFailurePoint::Renamed, failure)?;

    WorkspaceAssociation::new(id, final_repo).write(&association_path)?;
    sync_directory(&project_dir)?;
    #[cfg(test)]
    fail_after(CreationFailurePoint::Associated, failure)?;
    remove_journal(manager, id)?;
    WorkspaceManager::open_unlocked(manager, id)
}

fn validate_final_repository(path: &Path, id: uuid::Uuid) -> anyhow::Result<()> {
    let path = path.canonicalize()?;
    anyhow::ensure!(
        git::repository_root(&path)? == path,
        "planning destination is not a Git repository root"
    );
    let manifest = WorkspaceManifest::read(&path)?;
    anyhow::ensure!(
        manifest.workspace_id == id,
        "planning repository identity differs from its workspace directory"
    );
    git::status_paths(&path)?;
    Ok(())
}

fn remove_journal(manager: &WorkspaceManager, id: uuid::Uuid) -> anyhow::Result<()> {
    let path = journal_path(manager, id);
    match fs::remove_file(path) {
        Ok(()) => sync_directory(&manager.project_dir(id))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub(super) fn validate_title(title: &str) -> anyhow::Result<()> {
    let title = title.trim();
    anyhow::ensure!(!title.is_empty(), "workspace title must not be empty");
    anyhow::ensure!(
        title.chars().count() <= 120 && !title.chars().any(char::is_control),
        "workspace title must be at most 120 characters and contain no control characters"
    );
    Ok(())
}

#[cfg(test)]
fn fail_after(
    reached: CreationFailurePoint,
    requested: Option<CreationFailurePoint>,
) -> anyhow::Result<()> {
    if requested == Some(reached) {
        anyhow::bail!("injected workspace creation failure after {reached:?}");
    }
    Ok(())
}
