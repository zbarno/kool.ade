use crate::core::planning_sync::PlanningSyncStatus;

use super::WorkspaceManager;

impl WorkspaceManager {
    /// Fetch and apply queued semantic planning operations to the canonical remote.
    pub fn sync_planning(&self, id: uuid::Uuid) -> anyhow::Result<PlanningSyncStatus> {
        let workspace = self.open(id)?;
        crate::core::planning_sync::sync_workspace_at(&workspace.planning_repo, &self.home)
    }

    /// Read the last persisted synchronization state for a workspace.
    pub fn planning_sync_status(&self, id: uuid::Uuid) -> anyhow::Result<PlanningSyncStatus> {
        let workspace = self.open(id)?;
        crate::core::planning_sync::sync_status_at(&workspace.planning_repo, &self.home)
    }
}
