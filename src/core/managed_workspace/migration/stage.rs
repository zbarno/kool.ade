mod association;
mod populate;
mod recovery;
mod verification;

pub(super) use recovery::{rollback, workspace_for_checkout};

use association::{association_for, validate_migrated_final};
use verification::{git_text, verify_source_unchanged, write_conflict};

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use super::super::manager::sync_directory;

use super::{
    ManagedWorkspace, WorkspaceManager, canonical_code_checkout, inventory,
    journal::{self, Journal},
};

pub(super) fn migrate(
    manager: &WorkspaceManager,
    code_checkout: &Path,
) -> anyhow::Result<ManagedWorkspace> {
    migrate_inner(manager, code_checkout, None, None)
}

#[cfg(test)]
pub(super) fn migrate_with_failure(
    manager: &WorkspaceManager,
    code_checkout: &Path,
    point: super::MigrationFailurePoint,
) -> anyhow::Result<ManagedWorkspace> {
    migrate_inner(manager, code_checkout, Some(point.into()), None)
}

#[cfg(test)]
pub(super) fn migrate_with_registered_sources(
    manager: &WorkspaceManager,
    code_checkout: &Path,
    local_checkouts: &BTreeMap<String, PathBuf>,
) -> anyhow::Result<ManagedWorkspace> {
    migrate_inner(manager, code_checkout, None, Some(local_checkouts))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FailurePoint {
    JournalWritten,
    InitialCommitCreated,
    RepositoryRenamed,
    AssociationWritten,
}

#[cfg(test)]
impl From<super::MigrationFailurePoint> for FailurePoint {
    fn from(point: super::MigrationFailurePoint) -> Self {
        match point {
            super::MigrationFailurePoint::JournalWritten => Self::JournalWritten,
            super::MigrationFailurePoint::InitialCommitCreated => Self::InitialCommitCreated,
            super::MigrationFailurePoint::RepositoryRenamed => Self::RepositoryRenamed,
            super::MigrationFailurePoint::AssociationWritten => Self::AssociationWritten,
        }
    }
}

fn migrate_inner(
    manager: &WorkspaceManager,
    code_checkout: &Path,
    failure: Option<FailurePoint>,
    local_checkouts: Option<&BTreeMap<String, PathBuf>>,
) -> anyhow::Result<ManagedWorkspace> {
    let code_root = canonical_code_checkout(code_checkout)?;
    fs::create_dir_all(manager.home.join("projects"))?;
    let _global =
        super::super::locking::lock(&manager.home.join("projects/.workspace-migrate.lock"))?;
    if let Some(existing) = workspace_for_checkout(manager, &code_root)? {
        let path = journal::path(manager, &code_root);
        if path.is_file() {
            let mut journal: Journal = serde_json::from_slice(&fs::read(&path)?)?;
            journal.phase = "associated".into();
            journal.write(&path)?;
        }
        return Ok(existing);
    }

    let snapshot_result = match local_checkouts {
        Some(local_checkouts) => {
            populate::Snapshot::read_with_local_checkouts(code_root.clone(), local_checkouts)
        }
        None => populate::Snapshot::read(code_root.clone()),
    };
    let snapshot = match snapshot_result {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let conflicts = error
                .downcast_ref::<populate::SnapshotConflicts>()
                .map(|conflicts| conflicts.0.as_slice())
                .unwrap_or_default();
            write_conflict(manager, &code_root, None, &error.to_string(), conflicts)?;
            return Err(error);
        }
    };
    let inventory_sha256 =
        inventory::full_inventory_digest(&snapshot.artifacts, &snapshot.branches);
    let mut journal = Journal::load_or_create(
        manager,
        &code_root,
        journal::JournalSource {
            source_commit: &snapshot.commit,
            source_ref: &snapshot.git_ref,
            inventory_sha256: &inventory_sha256,
            legacy_workspace_id: snapshot.legacy_workspace_id,
            source_repositories: &snapshot.source_repositories,
            source_artifacts: &snapshot.artifacts,
            source_branches: &snapshot.branches,
        },
    )?;
    maybe_fail(failure, FailurePoint::JournalWritten)?;
    let project_dir = manager.project_dir(journal.workspace_id);
    fs::create_dir_all(&project_dir)?;
    let _workspace = super::super::locking::lock(&project_dir.join(".workspace.lock"))?;
    let journal_path = journal::path(manager, &code_root);
    let stage = project_dir.join("planning-repo.staging");
    let final_repo = manager.planning_repo_path(journal.workspace_id);

    if final_repo.exists() {
        validate_migrated_final(&final_repo, &journal)?;
    } else {
        populate::populate(manager, journal.workspace_id, &stage, &snapshot).inspect_err(
            |error| {
                let _ =
                    write_conflict(manager, &code_root, Some(&journal), &error.to_string(), &[]);
            },
        )?;
        verify_source_unchanged(&snapshot)?;
        let mut paths = super::super::git::status_paths(&stage)?;
        paths.retain(|path| path != ".git");
        if !paths.is_empty() {
            crate::core::gitops::commit(
                &stage,
                &format!(
                    "Kool.ad/e: migrate embedded planning {} ({})",
                    &journal.source_commit[..12],
                    journal.source_ref
                ),
                &paths,
            )?;
        }
        anyhow::ensure!(
            super::super::git::status_paths(&stage)?.is_empty(),
            "migration staging repository is not clean after commit"
        );
        journal.initial_commit = Some(git_text(&stage, &["rev-parse", "HEAD"])?);
        journal.phase = "verified".into();
        journal.write(&journal_path)?;
        maybe_fail(failure, FailurePoint::InitialCommitCreated)?;
        sync_directory(&stage)?;
        anyhow::ensure!(
            !final_repo.exists(),
            "planning destination already exists; it was not overwritten"
        );
        fs::rename(&stage, &final_repo)?;
        sync_directory(&project_dir)?;
        maybe_fail(failure, FailurePoint::RepositoryRenamed)?;
    }

    verify_source_unchanged(&snapshot)?;
    let association = association_for(&snapshot, journal.workspace_id, &final_repo)?;
    association.write(&manager.association_path(journal.workspace_id))?;
    sync_directory(&project_dir)?;
    maybe_fail(failure, FailurePoint::AssociationWritten)?;
    journal.phase = "associated".into();
    journal.write(&journal_path)?;
    super::super::WorkspaceManager::open_unlocked(manager, journal.workspace_id)
}

fn maybe_fail(failure: Option<FailurePoint>, point: FailurePoint) -> anyhow::Result<()> {
    if failure == Some(point) {
        anyhow::bail!("injected planning migration interruption at {point:?}");
    }
    Ok(())
}
