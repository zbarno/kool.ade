use super::*;

impl EngineDiscovery {
    /// Prepare only a registered local derivation. Unlike `prepare_image`, this
    /// path never pulls and requires the daemon's inspected image ID and labels
    /// to match the app-owned content identity and approved base digest.
    pub(crate) async fn prepare_derived_image(
        &self,
        image: &SelectedImage,
        content_identity: &str,
        base_digest: &str,
    ) -> Result<(), PrerequisiteFailure> {
        let observation = tokio::time::timeout(
            self.timeouts.image_preparation,
            self.api.inspect_derived_image(image),
        )
        .await
        .map_err(|_| operation_timeout("registered derived image inspection"))??;
        crate::execution::docker::images::verify_derived_image_observation(
            image,
            content_identity,
            base_digest,
            &observation,
        )
    }
}
