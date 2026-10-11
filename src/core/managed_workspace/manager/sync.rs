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

    pub fn resolve_planning_conflict(
        &self,
        id: uuid::Uuid,
        operation_id: uuid::Uuid,
        path: &str,
        choice: crate::core::planning_sync::PlanningConflictChoice,
        manual_value: Option<&str>,
    ) -> anyhow::Result<PlanningSyncStatus> {
        let workspace = self.open(id)?;
        crate::core::planning_sync::resolve_conflict_at(
            &workspace.planning_repo,
            &self.home,
            operation_id,
            path,
            choice,
            manual_value,
        )
    }

    pub fn cancel_pending_operation(
        &self,
        id: uuid::Uuid,
        operation_id: uuid::Uuid,
    ) -> anyhow::Result<PlanningSyncStatus> {
        let workspace = self.open(id)?;
        crate::core::planning_sync::cancel_pending_operation_at(
            &workspace.planning_repo,
            &self.home,
            operation_id,
        )
    }

    pub fn export_pending_planning(
        &self,
        id: uuid::Uuid,
        destination: &std::path::Path,
    ) -> anyhow::Result<PlanningSyncStatus> {
        let workspace = self.open(id)?;
        crate::core::planning_sync::export_pending_at(
            &workspace.planning_repo,
            &self.home,
            destination,
        )
    }
}
