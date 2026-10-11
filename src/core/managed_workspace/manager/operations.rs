use std::{fs, path::Path};

use super::super::{
    association::{CodeCheckout, WorkspaceAssociation},
    creation, git,
    manifest::WorkspaceManifest,
    repositories,
};
use super::{CodeRepositoryRegistration, ManagedWorkspace, WorkspaceManager, sync_directory};

impl WorkspaceManager {
    /// Associate an existing planning checkout without creating or replacing
    /// any repository. An existing local association is reused by identity.
    pub fn join(&self, planning_repo: &Path) -> anyhow::Result<ManagedWorkspace> {
        let (planning_repo, manifest) = validate_join_checkout(planning_repo)?;
        let _join_lock = super::super::locking::lock(&self.home.join("projects/.join.lock"))?;
        self.join_validated(planning_repo, manifest)
    }

    /// Complete a join while the caller holds the cross-process join lock.
    /// Remote joins use this after validating a clone in staging and before
    /// releasing that lock around the atomic move and association write.
    pub(super) fn join_while_locked(
        &self,
        planning_repo: &Path,
    ) -> anyhow::Result<ManagedWorkspace> {
        let (planning_repo, manifest) = validate_join_checkout(planning_repo)?;
        self.join_validated(planning_repo, manifest)
    }

    fn join_validated(
        &self,
        planning_repo: std::path::PathBuf,
        manifest: WorkspaceManifest,
    ) -> anyhow::Result<ManagedWorkspace> {
        let id = manifest.workspace_id;
        reject_conflicting_path(self, id, &planning_repo)?;
        if self.association_path(id).is_file() {
            let existing = self.open(id)?;
            anyhow::ensure!(
                existing.manifest.workspace_id == id,
                "local association is mapped to a different workspace identity"
            );
            return Ok(existing);
        }

        let project_dir = self.project_dir(id);
        if project_dir.exists() {
            creation::recover_id(self, id)?;
            if self.association_path(id).is_file() {
                return self.open(id);
            }
            let entries = fs::read_dir(&project_dir)?
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry.file_name() != ".workspace.lock" && entry.path() != planning_repo
                })
                .count();
            anyhow::ensure!(
                entries == 0,
                "workspace data directory already contains unrelated data"
            );
        } else {
            fs::create_dir_all(&project_dir)?;
        }
        let _lock = super::super::locking::lock(&project_dir.join(".workspace.lock"))?;
        if self.association_path(id).exists() {
            return WorkspaceManager::open_unlocked(self, id);
        }
        let association = WorkspaceAssociation::new(id, planning_repo);
        let association_path = self.association_path(id);
        if let Err(error) = association.write(&association_path) {
            return Err(remove_failed_association(&association_path, error));
        }
        if let Err(error) = sync_directory(&project_dir) {
            return Err(remove_failed_association(&association_path, error.into()));
        }
        match WorkspaceManager::open_unlocked(self, id) {
            Ok(workspace) => Ok(workspace),
            Err(error) => Err(remove_failed_association(&association_path, error)),
        }
    }

    pub fn register_code_repository(
        &self,
        id: uuid::Uuid,
        registration: CodeRepositoryRegistration,
    ) -> anyhow::Result<ManagedWorkspace> {
        self.open(id)?;
        let _lock = super::super::locking::lock(&self.project_dir(id).join(".workspace.lock"))?;
        let mut workspace = WorkspaceManager::open_unlocked(self, id)?;
        repositories::register(self, &mut workspace, registration)?;
        WorkspaceManager::open_unlocked(self, id)
    }

    /// Disconnects only this operator's path mapping. The shared repository
    /// registration and the code checkout are left untouched.
    pub fn disconnect_checkout(
        &self,
        id: uuid::Uuid,
        repository_id: &str,
    ) -> anyhow::Result<ManagedWorkspace> {
        let _lock = super::super::locking::lock(&self.project_dir(id).join(".workspace.lock"))?;
        let mut workspace = WorkspaceManager::open_unlocked(self, id)?;
        let before = workspace.association.code_checkouts.len();
        workspace
            .association
            .code_checkouts
            .retain(|checkout| checkout.repository_id != repository_id);
        if before != workspace.association.code_checkouts.len() {
            workspace.association.write(&self.association_path(id))?;
        }
        WorkspaceManager::open_unlocked(self, id)
    }

    pub fn map_code_checkout(
        &self,
        id: uuid::Uuid,
        repository_id: &str,
        checkout_path: &Path,
    ) -> anyhow::Result<ManagedWorkspace> {
        let _lock = super::super::locking::lock(&self.project_dir(id).join(".workspace.lock"))?;
        let mut workspace = WorkspaceManager::open_unlocked(self, id)?;
        let repository = workspace
            .repositories
            .repositories
            .iter()
            .find(|repository| repository.id == repository_id)
            .ok_or_else(|| anyhow::anyhow!("unknown repository ID {repository_id}"))?;
        let checkout = checkout_path.canonicalize()?;
        anyhow::ensure!(
            git::is_work_tree(&checkout),
            "code checkout is not a Git working tree"
        );
        if !repository.remote.is_empty() {
            anyhow::ensure!(
                git::origin(&checkout).as_deref() == Some(repository.remote.as_str()),
                "checkout origin does not match registered repository {repository_id}"
            );
        }
        let mapping = CodeCheckout {
            repository_id: repository_id.to_owned(),
            path: checkout,
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
        workspace.association.write(&self.association_path(id))?;
        WorkspaceManager::open_unlocked(self, id)
    }

    pub(crate) fn map_checkout(
        &self,
        id: uuid::Uuid,
        repository_id: &str,
        checkout_path: &Path,
    ) -> anyhow::Result<()> {
        self.map_code_checkout(id, repository_id, checkout_path)
            .map(|_| ())
    }

    pub(crate) fn checkout_path(
        &self,
        id: uuid::Uuid,
        repository_id: &str,
        expected_remote: &str,
    ) -> anyhow::Result<Option<std::path::PathBuf>> {
        if !self.association_path(id).is_file() {
            return Ok(None);
        }
        let workspace = WorkspaceManager::open_unlocked(self, id)?;
        let Some(checkout) = workspace
            .association
            .code_checkouts
            .iter()
            .find(|checkout| checkout.repository_id == repository_id)
        else {
            return Ok(None);
        };
        if !checkout.path.exists() {
            return Ok(None);
        }
        let path = checkout.path.canonicalize()?;
        anyhow::ensure!(
            git::is_work_tree(&path),
            "local checkout for {repository_id} is not a Git working tree"
        );
        anyhow::ensure!(
            expected_remote.is_empty() || git::origin(&path).as_deref() == Some(expected_remote),
            "local checkout for {repository_id} has a different origin"
        );
        Ok(Some(path))
    }
}

fn remove_failed_association(path: &Path, error: anyhow::Error) -> anyhow::Error {
    match fs::remove_file(path) {
        Ok(()) => error,
        Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => error,
        Err(cleanup) => error.context(format!(
            "failed to roll back incomplete workspace association at {}: {cleanup}",
            path.display()
        )),
    }
}

fn validate_join_checkout(
    planning_repo: &Path,
) -> anyhow::Result<(std::path::PathBuf, WorkspaceManifest)> {
    let planning_repo = planning_repo
        .canonicalize()
        .map_err(|error| anyhow::anyhow!("planning repository path is unreadable: {error}"))?;
    anyhow::ensure!(
        planning_repo.is_dir(),
        "planning repository is not a directory"
    );
    let root = git::repository_root(&planning_repo)
        .map_err(|error| anyhow::anyhow!("path is not a Git repository: {error}"))?;
    anyhow::ensure!(root == planning_repo, "path is not the Git repository root");
    let manifest = WorkspaceManifest::read(&planning_repo)
        .map_err(|error| anyhow::anyhow!("planning manifest is invalid: {error}"))?;
    let canonical_branch = manifest
        .canonical_ref
        .strip_prefix("refs/heads/")
        .expect("validated canonical planning branch");
    let current_branch = git::current_branch(&planning_repo)?;
    anyhow::ensure!(
        current_branch.as_deref() == Some(canonical_branch),
        "local planning checkout must be on its canonical branch {canonical_branch}; join by remote URL to fetch the shared canonical branch"
    );
    Ok((planning_repo, manifest))
}

fn reject_conflicting_path(
    manager: &WorkspaceManager,
    selected_id: uuid::Uuid,
    selected_path: &Path,
) -> anyhow::Result<()> {
    let projects = manager.home.join("projects");
    for entry in fs::read_dir(&projects)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let association_path = entry.path().join("association.json");
        if !association_path.is_file() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("local workspace association directory is unreadable"))?;
        let existing_id = uuid::Uuid::parse_str(&name).map_err(|error| {
            anyhow::anyhow!("local workspace association directory has an invalid UUID: {error}")
        })?;
        anyhow::ensure!(
            existing_id.hyphenated().to_string() == name,
            "local workspace association directory has a non-canonical UUID"
        );
        if existing_id == selected_id {
            continue;
        }
        let association =
            WorkspaceAssociation::read(&association_path, existing_id).map_err(|error| {
                anyhow::anyhow!("local workspace association is unreadable: {error}")
            })?;
        let associated_path = match association.planning_repo_path.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            associated_path != selected_path,
            "planning repository path is already associated with workspace {existing_id}, but its manifest declares {selected_id}"
        );
    }
    Ok(())
}
