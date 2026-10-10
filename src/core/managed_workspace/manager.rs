use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::project_repos::ProjectManifest,
    persistence::state_root,
};

use super::{association::WorkspaceAssociation, creation, git, manifest::WorkspaceManifest};

mod operations;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedWorkspace {
    pub id: uuid::Uuid,
    pub planning_repo: PathBuf,
    pub store: PlanningStore,
    pub manifest: WorkspaceManifest,
    pub association: WorkspaceAssociation,
    pub repositories: ProjectManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRepositoryRegistration {
    pub id: String,
    pub role: String,
    pub remote: Option<String>,
    pub checkout: PathBuf,
}

#[derive(Debug, Clone)]
pub struct WorkspaceManager {
    pub(super) home: PathBuf,
}

impl WorkspaceManager {
    pub fn new(state_root: impl Into<PathBuf>) -> Self {
        let state_root = state_root.into();
        let state_root = if state_root.is_absolute() {
            state_root
        } else {
            std::env::current_dir()
                .map(|current| current.join(&state_root))
                .unwrap_or(state_root)
        };
        Self { home: state_root }
    }

    pub fn from_state_root() -> Self {
        Self::new(state_root())
    }

    pub fn project_dir(&self, id: uuid::Uuid) -> PathBuf {
        self.home.join("projects").join(id.hyphenated().to_string())
    }

    pub fn planning_repo_path(&self, id: uuid::Uuid) -> PathBuf {
        self.project_dir(id).join("planning-repo")
    }

    pub fn association_path(&self, id: uuid::Uuid) -> PathBuf {
        self.project_dir(id).join("association.json")
    }

    pub fn create(&self, title: &str) -> anyhow::Result<ManagedWorkspace> {
        creation::create(self, title, None)
    }

    #[cfg(test)]
    pub(super) fn create_with_failure(
        &self,
        title: &str,
        point: creation::CreationFailurePoint,
    ) -> anyhow::Result<ManagedWorkspace> {
        creation::create(self, title, Some(point))
    }

    pub fn open(&self, id: uuid::Uuid) -> anyhow::Result<ManagedWorkspace> {
        creation::recover_id(self, id)?;
        anyhow::ensure!(
            self.association_path(id).is_file(),
            "workspace {id} has no local association; no replacement repository was created"
        );
        let project_dir = self.project_dir(id);
        let _lock = super::locking::lock(&project_dir.join(".workspace.lock"))?;
        Self::open_unlocked(self, id)
    }

    pub(super) fn open_unlocked(
        manager: &Self,
        id: uuid::Uuid,
    ) -> anyhow::Result<ManagedWorkspace> {
        let association_path = manager.association_path(id);
        anyhow::ensure!(
            association_path.is_file(),
            "workspace {id} has no local association; no replacement repository was created"
        );
        let association = WorkspaceAssociation::read(&association_path, id)
            .map_err(|error| anyhow::anyhow!("workspace association is unreadable: {error}"))?;
        let planning_repo = association
            .planning_repo_path
            .canonicalize()
            .map_err(|error| {
                anyhow::anyhow!("associated planning repository is missing or unreadable: {error}")
            })?;
        anyhow::ensure!(
            planning_repo.is_dir(),
            "associated planning repository is not a directory"
        );
        let repository_root = git::repository_root(&planning_repo)
            .map_err(|error| anyhow::anyhow!("associated path is not a Git repository: {error}"))?;
        anyhow::ensure!(
            repository_root == planning_repo,
            "associated path resolves to a different Git repository"
        );
        let manifest = WorkspaceManifest::read(&planning_repo)
            .map_err(|error| anyhow::anyhow!("planning manifest is invalid: {error}"))?;
        anyhow::ensure!(
            manifest.workspace_id == id,
            "planning manifest workspace identity mismatch"
        );
        // A status read proves Git can safely inspect this checkout. Dirty
        // planning work is valid and remains available to the operator.
        git::status_paths(&planning_repo)?;
        let store = PlanningStore::new(id, &planning_repo, StoreMode::ManagedLocal);
        anyhow::ensure!(
            store.root == planning_repo,
            "planning store root differs from the associated checkout"
        );
        let repositories = ProjectManifest::load(&store)?;
        repositories.validate()?;
        for checkout in &association.code_checkouts {
            anyhow::ensure!(
                repositories
                    .repositories
                    .iter()
                    .any(|repository| repository.id == checkout.repository_id),
                "local checkout references unregistered repository {}",
                checkout.repository_id
            );
        }
        Ok(ManagedWorkspace {
            id,
            planning_repo,
            store,
            manifest,
            association,
            repositories,
        })
    }
}

pub(super) fn sync_directory(path: &Path) -> io::Result<()> {
    match fs::File::open(path).and_then(|directory| directory.sync_all()) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::Unsupported | io::ErrorKind::PermissionDenied
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}
