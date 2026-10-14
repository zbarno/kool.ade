use super::*;
use crate::execution::remote::WorkspaceManifestV1;

impl StagingReceiver {
    /// Finalize only a complete workspace result for the expected task and
    /// source revision. This verifies all chunks and every file digest before
    /// returning a path that callers may pass to an explicit apply policy.
    pub fn finalize_workspace_result(
        &self,
        expected_workspace: &WorkspaceRef,
    ) -> anyhow::Result<VerifiedWorkspaceResult> {
        let TransferPayloadV1::WorkspaceResult(manifest) = &self.manifest.payload else {
            anyhow::bail!("transfer does not contain a complete workspace result");
        };
        validate_workspace_result(manifest, expected_workspace)?;
        let materialized_path = self.finalize()?;
        Ok(VerifiedWorkspaceResult {
            workspace: manifest.workspace.clone(),
            manifest: manifest.clone(),
            snapshot_generation: manifest.snapshot_generation.clone(),
            root_sha256: manifest.root_sha256.clone(),
            transfer_id: self.manifest.transfer_id.clone(),
            materialized_path,
        })
    }

    /// Pass a fully verified result to an application-owned apply transaction
    /// only after its claim or authorization fence is checked. The callback is
    /// responsible for its own unchanged-clone snapshot/CAS check and atomic
    /// apply semantics; this method never writes into the task clone.
    pub fn apply_workspace_result<T>(
        &self,
        expected_workspace: &WorkspaceRef,
        verify_fence: impl FnOnce() -> anyhow::Result<()>,
        apply: impl FnOnce(&VerifiedWorkspaceResult) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let result = self.finalize_workspace_result(expected_workspace)?;
        verify_fence()?;
        apply(&result)
    }
}

fn validate_workspace_result(
    manifest: &WorkspaceManifestV1,
    expected: &WorkspaceRef,
) -> anyhow::Result<()> {
    manifest.validate()?;
    expected.validate()?;
    anyhow::ensure!(
        manifest.workspace == *expected,
        "workspace result task identity or source SHA does not match"
    );
    Ok(())
}
