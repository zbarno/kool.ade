use crate::{execution::session::ExecutionSession, project_runtime::RuntimeOperation};

use super::ProjectRuntimeExecutor;

impl<'store, 'planning> ProjectRuntimeExecutor<'store, 'planning> {
    /// Checks whether persisted recipes have local approval provenance for this
    /// exact task workspace. This method never creates or widens a session.
    pub fn validate_candidate(
        &self,
        session: &ExecutionSession,
        target_id: &str,
        operation: RuntimeOperation,
    ) -> anyhow::Result<(
        crate::project_runtime::ProjectCapabilityManifestV1,
        crate::project_runtime::AppTargetV1,
    )> {
        let manifest = self
            .store
            .load_manifest()?
            .ok_or_else(|| anyhow::anyhow!("project runtime manifest is missing"))?;
        let target = manifest
            .candidate_targets
            .iter()
            .find(|candidate| candidate.target_id == target_id)
            .ok_or_else(|| anyhow::anyhow!("runtime target is not in the approved manifest"))?;
        self.store
            .verify_execution_provenance(&manifest, target, operation)
            .map_err(|error| anyhow::anyhow!("runtime execution provenance is invalid: {error}"))?;
        let target = super::validate_manifest_candidate(
            &manifest,
            session.workspace(),
            target_id,
            operation,
            true,
        )?;
        Ok((manifest, target))
    }
}
