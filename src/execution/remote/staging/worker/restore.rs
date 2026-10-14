use super::*;

impl WorkerStager {
    pub(super) fn restore_workspaces(&self) -> anyhow::Result<()> {
        let entries = fs::read_dir(&self.staging_root)?;
        let mut workspaces = self
            .workspaces
            .lock()
            .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?;
        let mut ambiguous = std::collections::HashSet::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let manifest_path = entry.path().join("manifest.json");
            let Ok(bytes) = read_bounded(&manifest_path, 512 * 1024) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_slice::<TransferManifestV1>(&bytes) else {
                continue;
            };
            if manifest.validate().is_err()
                || manifest.destination_identity != format!("worker:{}", self.worker_id)
                || !matches!(manifest.payload, TransferPayloadV1::Workspace(_))
                || fs::symlink_metadata(entry.path().join("complete.json")).is_err()
            {
                continue;
            }
            let TransferPayloadV1::Workspace(snapshot) = &manifest.payload else {
                continue;
            };
            let _shared = self.lock_registry(&snapshot.workspace, false)?;
            let selected = self.selected_workspace(&snapshot.workspace)?;
            if selected
                .as_ref()
                .is_some_and(|selected| selected != snapshot)
                || ambiguous.contains(&snapshot.workspace)
            {
                continue;
            }
            if selected.is_none() && workspaces.contains_key(&snapshot.workspace) {
                // Legacy state with multiple generations has no authoritative
                // selection. Deny that identity rather than guessing by readdir.
                workspaces.remove(&snapshot.workspace);
                ambiguous.insert(snapshot.workspace.clone());
                continue;
            }
            let path = self.workspace_path(snapshot)?;
            let Ok(actual) = crate::execution::remote::WorkspaceManifestV1::capture_result(
                snapshot.workspace.clone(),
                &path,
                snapshot.snapshot_generation.clone(),
            ) else {
                continue;
            };
            if actual.entries == snapshot.entries
                && actual.excluded_paths == snapshot.excluded_paths
                && actual.root_sha256 == snapshot.root_sha256
            {
                workspaces.insert(
                    snapshot.workspace.clone(),
                    StagedWorkspace {
                        path,
                        snapshot_generation: snapshot.snapshot_generation.clone(),
                        reservations: 0,
                        leases: Default::default(),
                        executions: Default::default(),
                    },
                );
            }
        }
        Ok(())
    }
}
