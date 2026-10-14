use super::*;

impl WorkerStager {
    pub fn create_workspace_result(
        &self,
        execution_id: &str,
        workspace: &WorkspaceRef,
        snapshot_generation: &str,
        destination_identity: &str,
    ) -> anyhow::Result<TransferManifestV1> {
        let staged = self
            .workspaces
            .lock()
            .map_err(|_| anyhow::anyhow!("worker workspace registry is unavailable"))?
            .get(workspace)
            .map(|staged| (staged.path.clone(), staged.snapshot_generation.clone()))
            .ok_or_else(|| anyhow::anyhow!("workspace snapshot is not staged"))?;
        anyhow::ensure!(
            staged.1 == snapshot_generation,
            "workspace result generation does not match its input snapshot"
        );
        let snapshot = crate::execution::remote::WorkspaceManifestV1::capture_result(
            workspace.clone(),
            &staged.0,
            snapshot_generation.to_owned(),
        )
        .map_err(|_| anyhow::anyhow!("worker workspace result could not be captured"))?;
        let manifest = TransferManifestV1::workspace_result(
            format!("worker:{}", self.worker_id),
            destination_identity,
            snapshot,
        )?;
        let directory = self
            .staging_root
            .join("outgoing")
            .join(&manifest.transfer_id);
        create_private_dir(&directory)?;
        let manifest_path = directory.join("manifest.json");
        crate::artifacts::atomic_create_bytes(&manifest_path, &serde_json::to_vec(&manifest)?)?;
        set_private_file(&manifest_path)?;
        let execution_path = directory.join("execution-id.json");
        crate::artifacts::atomic_create_bytes(&execution_path, &serde_json::to_vec(execution_id)?)?;
        set_private_file(&execution_path)?;
        Ok(manifest)
    }

    pub fn read_result_chunk(
        &self,
        execution_id: &str,
        transfer_id: &str,
        expected_workspace: &WorkspaceRef,
        path: &str,
        offset: u64,
    ) -> anyhow::Result<TransferChunkV1> {
        let manifest_path = self
            .staging_root
            .join("outgoing")
            .join(transfer_id)
            .join("manifest.json");
        let manifest: TransferManifestV1 =
            serde_json::from_slice(&read_bounded(&manifest_path, 512 * 1024)?)?;
        manifest.validate()?;
        let execution_path = manifest_path
            .parent()
            .expect("manifest has parent")
            .join("execution-id.json");
        let stored_execution: String =
            serde_json::from_slice(&read_bounded(&execution_path, 4096)?)?;
        anyhow::ensure!(
            manifest.transfer_id == transfer_id
                && stored_execution == execution_id
                && manifest.source_identity == format!("worker:{}", self.worker_id),
            "workspace result transfer identity is invalid"
        );
        let workspace = manifest
            .workspace_ref()
            .ok_or_else(|| anyhow::anyhow!("workspace result identity is missing"))?;
        anyhow::ensure!(
            workspace == expected_workspace,
            "workspace result task identity differs"
        );
        let generation = match &manifest.payload {
            TransferPayloadV1::WorkspaceResult(result) => &result.snapshot_generation,
            _ => anyhow::bail!("transfer is not a workspace result"),
        };
        let staged_path = self.workspace_path_for(workspace, generation);
        super::super::sender::read_file_chunk(&manifest, &staged_path, path, offset)
    }
}
