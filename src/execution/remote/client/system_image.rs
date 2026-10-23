use super::*;

impl RemoteWorkerClient {
    pub(in crate::execution::remote) fn system_image_capability(
        &self,
        transfer_id: &str,
        proposal: &crate::execution::docker::image_extension::SystemImageExtensionRequest,
        decision: crate::harness::DependencyDecision,
        workspace: &crate::execution::contract::WorkspaceRef,
        manifest: &[crate::execution::docker::image_extension::builder::PackageArtifactIdentity],
    ) -> Result<String, ExecutionError> {
        crate::execution::docker::image_extension::sign_system_image_capability(
            &self.system_image_capability_key,
            transfer_id,
            proposal,
            decision,
            workspace,
            manifest,
        )
        .map_err(|_| incompatible())
    }

    pub(super) fn system_image_key_hex(&self) -> zeroize::Zeroizing<String> {
        encode_capability_key(&self.system_image_capability_key)
    }
}
