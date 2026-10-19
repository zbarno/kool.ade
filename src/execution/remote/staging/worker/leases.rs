use super::*;

/// Prevent generation changes between resolving a workspace and issuing its lease.
pub struct WorkspaceReservation<'a> {
    stager: &'a WorkerStager,
    workspace: WorkspaceRef,
    lock: Option<fs::File>,
}

impl WorkerStager {
    pub fn reserve_workspace(
        &self,
        workspace: &WorkspaceRef,
    ) -> anyhow::Result<WorkspaceReservation<'_>> {
        let lock = self.lock_registry(workspace, false)?;
        let current = self.selected_workspace(workspace)?;
        let mut workspaces = self
            .workspaces
            .lock()
            .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?;
        if let Some(current) = current {
            let path = self.workspace_path(&current)?;
            if workspaces
                .get(workspace)
                .is_none_or(|staged| staged.snapshot_generation != current.snapshot_generation)
            {
                anyhow::ensure!(
                    workspaces
                        .get(workspace)
                        .is_none_or(|staged| staged.reservations == 0
                            && staged.leases.is_empty()
                            && staged.executions.is_empty()),
                    "cached workspace generation still has owners"
                );
                workspaces.insert(
                    workspace.clone(),
                    StagedWorkspace {
                        path,
                        snapshot_generation: current.snapshot_generation,
                        reservations: 0,
                        leases: Default::default(),
                        executions: Default::default(),
                    },
                );
            }
        }
        let staged = workspaces
            .get_mut(workspace)
            .ok_or_else(|| anyhow::anyhow!("workspace is not staged"))?;
        staged.reservations += 1;
        Ok(WorkspaceReservation {
            stager: self,
            workspace: workspace.clone(),
            lock: Some(lock),
        })
    }

    pub fn bind_execution(&self, lease_id: &str, execution_id: &str) -> anyhow::Result<()> {
        let mut workspaces = self
            .workspaces
            .lock()
            .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?;
        let staged = workspaces
            .values_mut()
            .find(|staged| staged.leases.contains_key(lease_id))
            .ok_or_else(|| anyhow::anyhow!("execution lease is not reserved"))?;
        let lock = staged
            .leases
            .get(lease_id)
            .expect("lease was located")
            .try_clone()?;
        staged.executions.insert(execution_id.to_owned(), lock);
        Ok(())
    }

    pub fn release_execution(&self, execution_id: &str) -> anyhow::Result<()> {
        {
            let mut workspaces = self
                .workspaces
                .lock()
                .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?;
            for staged in workspaces.values_mut() {
                staged.executions.remove(execution_id);
            }
        }
        self.private_files.cleanup(execution_id, true)
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
    pub fn bind_lease(mut self, lease_id: &str) -> anyhow::Result<()> {
        let mut workspaces = self
            .stager
            .workspaces
            .lock()
            .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?;
        let staged = workspaces
            .get_mut(&self.workspace)
            .ok_or_else(|| anyhow::anyhow!("workspace reservation was lost"))?;
        staged.leases.insert(
            lease_id.to_owned(),
            self.lock.take().expect("reservation owns lock"),
        );
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
