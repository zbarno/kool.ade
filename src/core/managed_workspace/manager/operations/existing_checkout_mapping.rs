use std::path::Path;

use super::super::super::{association::CodeCheckout, git, locking, repositories};
use super::{CodeRepositoryRegistration, ManagedWorkspace, WorkspaceManager};

impl WorkspaceManager {
    /// Reuse an existing path mapping or add one only if its repository ID is
    /// still unmapped. The association is reloaded while holding the workspace
    /// lock so a stale onboarding snapshot cannot replace a newer checkout.
    pub(crate) fn map_existing_checkout_if_available(
        &self,
        id: uuid::Uuid,
        repository_id: Option<&str>,
        checkout_path: &Path,
    ) -> anyhow::Result<Option<ManagedWorkspace>> {
        let _lock = locking::lock(&self.project_dir(id).join(".workspace.lock"))?;
        let mut workspace = WorkspaceManager::open_unlocked(self, id)?;
        let checkout = checkout_path.canonicalize()?;

        if workspace
            .association
            .code_checkouts
            .iter()
            .any(|mapping| mapping.path == checkout)
        {
            return WorkspaceManager::open_unlocked(self, id).map(Some);
        }
        let Some(repository_id) = repository_id else {
            return Ok(None);
        };
        if workspace
            .association
            .code_checkouts
            .iter()
            .any(|mapping| mapping.repository_id == repository_id)
        {
            return Ok(None);
        }

        let repository = workspace
            .repositories
            .repositories
            .iter()
            .find(|repository| repository.id == repository_id)
            .ok_or_else(|| anyhow::anyhow!("unknown repository ID {repository_id}"))?;
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

        workspace.association.code_checkouts.push(CodeCheckout {
            repository_id: repository_id.to_owned(),
            path: checkout,
        });
        workspace.association.write(&self.association_path(id))?;
        WorkspaceManager::open_unlocked(self, id).map(Some)
    }

    /// Register a newly selected checkout only while the request's repository
    /// ID remains unmapped. A retry must never replace a mapping made by a
    /// different window after the original onboarding request was saved.
    pub(crate) fn register_existing_checkout_if_available(
        &self,
        id: uuid::Uuid,
        registration: CodeRepositoryRegistration,
    ) -> anyhow::Result<Option<ManagedWorkspace>> {
        let _lock = locking::lock(&self.project_dir(id).join(".workspace.lock"))?;
        let mut workspace = WorkspaceManager::open_unlocked(self, id)?;
        let checkout = registration.checkout.canonicalize()?;

        if workspace
            .association
            .code_checkouts
            .iter()
            .any(|mapping| mapping.path == checkout)
        {
            return WorkspaceManager::open_unlocked(self, id).map(Some);
        }
        if workspace
            .association
            .code_checkouts
            .iter()
            .any(|mapping| mapping.repository_id == registration.id)
        {
            return Ok(None);
        }

        repositories::register(self, &mut workspace, registration)?;
        WorkspaceManager::open_unlocked(self, id).map(Some)
    }
}
