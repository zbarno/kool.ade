use super::*;

/// Prevent generation changes between resolving a workspace and issuing its lease.
pub struct WorkspaceReservation<'a> {
    stager: &'a WorkerStager,
    workspace: WorkspaceRef,
}

impl WorkerStager {
    pub fn reserve_workspace(
        &self,
        workspace: &WorkspaceRef,
    ) -> anyhow::Result<WorkspaceReservation<'_>> {
        let mut workspaces = self
            .workspaces
            .lock()
            .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?;
        let staged = workspaces
            .get_mut(workspace)
            .ok_or_else(|| anyhow::anyhow!("workspace is not staged"))?;
        staged.reservations += 1;
        Ok(WorkspaceReservation {
            stager: self,
            workspace: workspace.clone(),
        })
    }

    pub fn bind_execution(&self, lease_id: &str, execution_id: &str) -> anyhow::Result<()> {
        let mut workspaces = self
            .workspaces
            .lock()
            .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?;
        let staged = workspaces
            .values_mut()
            .find(|staged| staged.leases.contains(lease_id))
            .ok_or_else(|| anyhow::anyhow!("execution lease is not reserved"))?;
        staged.executions.insert(execution_id.to_owned());
        Ok(())
    }

    pub fn release_execution(&self, execution_id: &str) -> anyhow::Result<()> {
        let mut workspaces = self
            .workspaces
            .lock()
            .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?;
        for staged in workspaces.values_mut() {
            staged.executions.remove(execution_id);
        }
        Ok(())
    }

    pub fn release_workspace(&self, lease_id: &str) -> anyhow::Result<()> {
        let mut workspaces = self
            .workspaces
            .lock()
            .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?;
        for staged in workspaces.values_mut() {
            staged.leases.remove(lease_id);
        }
        Ok(())
    }
}

impl WorkspaceReservation<'_> {
    pub fn bind_lease(&self, lease_id: &str) -> anyhow::Result<()> {
        let mut workspaces = self
            .stager
            .workspaces
            .lock()
            .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?;
        let staged = workspaces
            .get_mut(&self.workspace)
            .ok_or_else(|| anyhow::anyhow!("workspace reservation was lost"))?;
        staged.leases.insert(lease_id.to_owned());
        Ok(())
    }
}

impl Drop for WorkspaceReservation<'_> {
    fn drop(&mut self) {
        if let Ok(mut workspaces) = self.stager.workspaces.lock()
            && let Some(staged) = workspaces.get_mut(&self.workspace)
        {
            staged.reservations = staged.reservations.saturating_sub(1);
        }
    }
}
