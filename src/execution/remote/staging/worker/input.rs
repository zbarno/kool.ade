use super::{
    materialize::{copy_workspace_snapshot, initialize_isolated_git},
    *,
};

impl WorkerStager {
    pub fn begin(&self, manifest: TransferManifestV1) -> anyhow::Result<()> {
        anyhow::ensure!(
            manifest.destination_identity == format!("worker:{}", self.worker_id),
            "workspace transfer is addressed to another worker"
        );
        anyhow::ensure!(
            matches!(manifest.payload, TransferPayloadV1::Workspace(_)),
            "worker accepts only workspace input transfers"
        );
        let source = manifest.source_identity.clone();
        let destination = manifest.destination_identity.clone();
        let receiver =
            StagingReceiver::begin(&self.staging_root, manifest.clone(), &source, &destination)?;
        let mut receivers = self
            .receivers
            .lock()
            .map_err(|_| anyhow::anyhow!("worker staging lock is unavailable"))?;
        if let Some(existing) = receivers.get(&manifest.transfer_id) {
            anyhow::ensure!(
                existing.manifest() == &manifest,
                "transfer ID is already bound to different workspace content"
            );
        } else {
            receivers.insert(manifest.transfer_id, receiver);
        }
        Ok(())
    }

    pub fn accept_chunk(&self, chunk: &TransferChunkV1) -> anyhow::Result<()> {
        let mut receivers = self
            .receivers
            .lock()
            .map_err(|_| anyhow::anyhow!("worker staging lock is unavailable"))?;
        let receiver = receivers
            .get_mut(&chunk.transfer_id)
            .ok_or_else(|| anyhow::anyhow!("workspace transfer has not begun"))?;
        receiver.accept_chunk(chunk)
    }

    pub fn finalize(&self, transfer_id: &str) -> anyhow::Result<()> {
        let receivers = self
            .receivers
            .lock()
            .map_err(|_| anyhow::anyhow!("worker staging lock is unavailable"))?;
        let receiver = receivers
            .get(transfer_id)
            .ok_or_else(|| anyhow::anyhow!("workspace transfer has not begun"))?;
        let TransferPayloadV1::Workspace(workspace) = &receiver.manifest().payload else {
            anyhow::bail!("worker transfer is not a workspace snapshot");
        };
        let materialized = receiver.finalize()?;
        let destination = self.workspace_path(workspace)?;
        if destination.exists() {
            let workspaces = self
                .workspaces
                .lock()
                .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?;
            if workspaces
                .get(&workspace.workspace)
                .is_some_and(|staged| staged.path == destination)
            {
                return Ok(());
            }
            anyhow::bail!("worker workspace snapshot already exists");
        }
        create_private_dir(destination.parent().expect("workspace has parent"))?;
        copy_workspace_snapshot(workspace, &materialized, &destination)?;
        if let Err(error) = initialize_isolated_git(&destination, &self.staging_root) {
            let _ = fs::remove_dir_all(&destination);
            return Err(error);
        }
        let mut workspaces = self
            .workspaces
            .lock()
            .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?;
        if workspaces
            .insert(
                workspace.workspace.clone(),
                StagedWorkspace {
                    path: destination,
                    snapshot_generation: workspace.snapshot_generation.clone(),
                },
            )
            .is_some()
        {
            anyhow::bail!("worker workspace identity is already staged");
        }
        Ok(())
    }
}
