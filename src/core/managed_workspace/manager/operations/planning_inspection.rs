use super::super::WorkspaceManager;
use std::path::Path;

impl WorkspaceManager {
    /// Validate an existing local planning checkout before the UI runs any
    /// separately approved code repository operation.
    pub fn inspect_planning_repository(&self, planning_repo: &Path) -> anyhow::Result<uuid::Uuid> {
        let (planning_repo, manifest) = super::validate_join_checkout(planning_repo)?;
        super::reject_conflicting_path(self, manifest.workspace_id, &planning_repo)?;
        if self.association_path(manifest.workspace_id).is_file() {
            let workspace = self.open(manifest.workspace_id)?;
            anyhow::ensure!(
                workspace.planning_repo == planning_repo,
                "selected planning repository conflicts with its existing local association"
            );
        }
        Ok(manifest.workspace_id)
    }
}
